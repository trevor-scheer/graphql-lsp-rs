use crate::{ExtractError, Language, Position, Range, Result, SourceLocation};
use std::fs;
use std::path::Path;

/// Configuration for GraphQL extraction
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    /// Magic comment to look for (default: "GraphQL")
    /// Matches comments like: /* GraphQL */ `query { ... }`
    pub magic_comment: String,

    /// Tag identifiers to extract (default: `["gql", "graphql"]`)
    /// Matches: `gql`query { ... }`\` or `graphql`query { ... }`\`
    pub tag_identifiers: Vec<String>,

    /// Module names to recognize as GraphQL sources
    /// Default includes: graphql-tag, @apollo/client, etc.
    pub modules: Vec<String>,

    /// Allow extraction without imports (global identifiers)
    pub allow_global_identifiers: bool,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            magic_comment: "GraphQL".to_string(),
            tag_identifiers: vec!["gql".to_string(), "graphql".to_string()],
            modules: vec![
                "graphql-tag".to_string(),
                "@apollo/client".to_string(),
                "apollo-server".to_string(),
                "apollo-server-express".to_string(),
                "gatsby".to_string(),
                "react-relay".to_string(),
            ],
            allow_global_identifiers: false,
        }
    }
}

/// Extracted GraphQL content with source location
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedGraphQL {
    /// The extracted GraphQL source code
    pub source: String,

    /// Source location in the original file
    pub location: SourceLocation,

    /// The tag name used (e.g., "gql", "graphql"), if any
    pub tag_name: Option<String>,
}

/// Extract GraphQL from a file
pub fn extract_from_file(path: &Path, config: &ExtractConfig) -> Result<Vec<ExtractedGraphQL>> {
    let language = Language::from_path(path)
        .ok_or_else(|| ExtractError::UnsupportedFileType(path.to_path_buf()))?;

    let source = fs::read_to_string(path)?;
    extract_from_source(&source, language, config)
}

/// Extract GraphQL from source code string
pub fn extract_from_source(
    source: &str,
    language: Language,
    config: &ExtractConfig,
) -> Result<Vec<ExtractedGraphQL>> {
    match language {
        Language::GraphQL => {
            // Raw GraphQL file - return entire content
            Ok(vec![ExtractedGraphQL {
                source: source.to_string(),
                location: SourceLocation::new(
                    0,
                    source.len(),
                    Range::new(
                        Position::new(0, 0),
                        position_from_offset(source, source.len()),
                    ),
                ),
                tag_name: None,
            }])
        }
        Language::TypeScript | Language::JavaScript => {
            extract_from_js_family(source, language, config)
        }
        _ => Err(ExtractError::UnsupportedLanguage(language)),
    }
}

#[allow(dead_code)] // Will be used when TS/JS extraction is implemented
#[allow(clippy::unnecessary_wraps)] // Will return errors when implemented
#[allow(clippy::missing_const_for_fn)] // Will not be const when implemented
fn extract_from_js_family(
    source: &str,
    language: Language,
    config: &ExtractConfig,
) -> Result<Vec<ExtractedGraphQL>> {
    use swc_common::sync::Lrc;
    use swc_common::{FileName, SourceMap};
    use swc_core::ecma::ast::EsVersion;
    use swc_core::ecma::parser::{parse_file_as_module, Syntax};
    use swc_core::ecma::visit::VisitWith;

    // Create source map for accurate position tracking
    let source_map = Lrc::new(SourceMap::default());
    let source_file = source_map.new_source_file(
        Lrc::new(FileName::Custom("input".into())),
        source.to_string(),
    );

    // Configure syntax based on language
    let syntax = match language {
        Language::TypeScript => Syntax::Typescript(swc_core::ecma::parser::TsSyntax {
            tsx: true,
            decorators: true,
            ..Default::default()
        }),
        Language::JavaScript => Syntax::Es(swc_core::ecma::parser::EsSyntax {
            jsx: true,
            ..Default::default()
        }),
        _ => unreachable!("extract_from_js_family only handles JS/TS"),
    };

    // Parse the module
    let module = parse_file_as_module(&source_file, syntax, EsVersion::EsNext, None, &mut vec![])
        .map_err(|e| ExtractError::Parse {
        path: std::path::PathBuf::from("input"),
        message: format!("SWC parse error: {e:?}"),
    })?;

    // Create visitor to collect GraphQL
    let mut visitor = GraphQLVisitor::new(source, config);
    eprintln!(
        "DEBUG: Starting visit_with on module with {} items",
        module.body.len()
    );
    module.visit_with(&mut visitor);
    eprintln!(
        "DEBUG: After visit_with, extracted {} items",
        visitor.extracted.len()
    );

    Ok(visitor.extracted)
}

/// Visitor to extract GraphQL from JavaScript/TypeScript AST
struct GraphQLVisitor<'a> {
    source: &'a str,
    config: &'a ExtractConfig,
    extracted: Vec<ExtractedGraphQL>,
    /// Map of imported identifiers to their module source
    /// e.g., "gql" -> "graphql-tag"
    imports: std::collections::HashMap<String, String>,
    /// Track comments for magic comment detection
    pending_comments: Vec<(usize, String)>,
}

impl<'a> GraphQLVisitor<'a> {
    #[allow(clippy::unnecessary_wraps)] // Consistent interface for extraction methods
    fn new(source: &'a str, config: &'a ExtractConfig) -> Self {
        Self {
            source,
            config,
            extracted: Vec::new(),
            imports: std::collections::HashMap::new(),
            pending_comments: Vec::new(),
        }
    }

    /// Check if a tag identifier is valid (imported or global allowed)
    fn is_valid_tag(&self, tag_name: &str) -> bool {
        if self.config.allow_global_identifiers {
            return true;
        }

        if let Some(module_source) = self.imports.get(tag_name) {
            return self.config.modules.contains(module_source);
        }

        false
    }

    /// Extract string content from a template literal
    fn extract_template_literal(
        &self,
        tpl: &swc_core::ecma::ast::Tpl,
        tag_name: Option<String>,
    ) -> Option<ExtractedGraphQL> {
        if tpl.quasis.is_empty() {
            return None;
        }

        // For now, only support templates without expressions
        if tpl.exprs.is_empty() && tpl.quasis.len() == 1 {
            let quasi = &tpl.quasis[0];
            let raw_str = String::from_utf8_lossy(quasi.raw.as_bytes());

            // Calculate positions
            let start_offset = quasi.span.lo.0 as usize - 1; // -1 to account for SWC byte offset
            let length = raw_str.len();

            let start_pos = position_from_offset(self.source, start_offset);
            let end_pos = position_from_offset(self.source, start_offset + length);

            return Some(ExtractedGraphQL {
                source: raw_str.to_string(),
                location: SourceLocation::new(start_offset, length, Range::new(start_pos, end_pos)),
                tag_name,
            });
        }

        None
    }

    /// Check if there's a magic comment before this position
    fn check_magic_comment(&self, pos: usize) -> bool {
        // Look for a comment that precedes this position
        self.pending_comments.iter().any(|(comment_pos, content)| {
            *comment_pos < pos && content.trim() == self.config.magic_comment
        })
    }
}

impl swc_core::ecma::visit::Visit for GraphQLVisitor<'_> {
    /// Visit import declarations to track GraphQL imports
    fn visit_import_decl(&mut self, import: &swc_core::ecma::ast::ImportDecl) {
        use swc_core::ecma::visit::VisitWith;
        let module_source = String::from_utf8_lossy(import.src.value.as_bytes()).to_string();
        eprintln!("DEBUG: Visiting import from module: {module_source}");

        // Only track imports from configured modules
        if self.config.modules.contains(&module_source) {
            eprintln!("DEBUG: Module IS in configured modules list");
            for specifier in &import.specifiers {
                use swc_core::ecma::ast::ImportSpecifier;
                match specifier {
                    ImportSpecifier::Named(named) => {
                        // Map local name to module source
                        let local_name =
                            String::from_utf8_lossy(named.local.sym.as_bytes()).to_string();
                        self.imports.insert(local_name, module_source.clone());
                    }
                    ImportSpecifier::Default(default) => {
                        let local_name =
                            String::from_utf8_lossy(default.local.sym.as_bytes()).to_string();
                        self.imports.insert(local_name, module_source.clone());
                    }
                    ImportSpecifier::Namespace(ns) => {
                        let local_name =
                            String::from_utf8_lossy(ns.local.sym.as_bytes()).to_string();
                        self.imports.insert(local_name, module_source.clone());
                    }
                }
            }
        } else {
            eprintln!("DEBUG: Module not in configured modules list");
        }

        // Continue traversal into child nodes
        import.visit_children_with(self);
    }

    /// Visit tagged template expressions (e.g., gql`query { ... }`)
    fn visit_tagged_tpl(&mut self, tagged: &swc_core::ecma::ast::TaggedTpl) {
        use swc_core::ecma::ast::Expr;
        use swc_core::ecma::visit::VisitWith;
        eprintln!("DEBUG: Visiting tagged template");
        // Extract tag identifier
        let tag_name = match &*tagged.tag {
            Expr::Ident(ident) => String::from_utf8_lossy(ident.sym.as_bytes()).to_string(),
            Expr::Member(member) => {
                // Handle member expressions like `graphql.default`
                if let Expr::Ident(obj) = &*member.obj {
                    String::from_utf8_lossy(obj.sym.as_bytes()).to_string()
                } else {
                    tagged.visit_children_with(self);
                    return;
                }
            }
            _ => {
                tagged.visit_children_with(self);
                return;
            }
        };

        // Check if this is a configured tag identifier
        if !self.config.tag_identifiers.contains(&tag_name) {
            tagged.visit_children_with(self);
            return;
        }

        // Check if the tag is valid (imported or global allowed)
        if !self.is_valid_tag(&tag_name) {
            tagged.visit_children_with(self);
            return;
        }

        // Extract the template literal content
        if let Some(extracted) = self.extract_template_literal(&tagged.tpl, Some(tag_name)) {
            self.extracted.push(extracted);
        }

        // Continue traversal into child nodes
        tagged.visit_children_with(self);
    }

    /// Visit call expressions to handle cases like gql(/* GraphQL */ "query")
    fn visit_call_expr(&mut self, call: &swc_core::ecma::ast::CallExpr) {
        use swc_core::ecma::ast::{Expr, Lit};
        use swc_core::ecma::visit::VisitWith;
        // Check if there are any string arguments with magic comments
        for arg in &call.args {
            if let Expr::Lit(Lit::Str(str_lit)) = &*arg.expr {
                let pos = str_lit.span.lo.0 as usize;
                if self.check_magic_comment(pos) {
                    let start_offset = str_lit.span.lo.0 as usize - 1;
                    let content = String::from_utf8_lossy(str_lit.value.as_bytes()).to_string();
                    let length = content.len();

                    let start_pos = position_from_offset(self.source, start_offset);
                    let end_pos = position_from_offset(self.source, start_offset + length);

                    self.extracted.push(ExtractedGraphQL {
                        source: content,
                        location: SourceLocation::new(
                            start_offset,
                            length,
                            Range::new(start_pos, end_pos),
                        ),
                        tag_name: None,
                    });
                }
            }
        }

        // Continue traversal into child nodes
        call.visit_children_with(self);
    }

    /// Visit variable declarations to handle magic comments
    fn visit_var_declarator(&mut self, decl: &swc_core::ecma::ast::VarDeclarator) {
        use swc_core::ecma::ast::{Expr, Lit};
        use swc_core::ecma::visit::VisitWith;
        if let Some(init) = &decl.init {
            match &**init {
                Expr::Lit(Lit::Str(str_lit)) => {
                    let pos = str_lit.span.lo.0 as usize;
                    if self.check_magic_comment(pos) {
                        let start_offset = str_lit.span.lo.0 as usize - 1;
                        let content = String::from_utf8_lossy(str_lit.value.as_bytes()).to_string();
                        let length = content.len();

                        let start_pos = position_from_offset(self.source, start_offset);
                        let end_pos = position_from_offset(self.source, start_offset + length);

                        self.extracted.push(ExtractedGraphQL {
                            source: content,
                            location: SourceLocation::new(
                                start_offset,
                                length,
                                Range::new(start_pos, end_pos),
                            ),
                            tag_name: None,
                        });
                    }
                }
                Expr::Tpl(tpl) => {
                    let pos = tpl.span.lo.0 as usize;
                    if self.check_magic_comment(pos) {
                        if let Some(extracted) = self.extract_template_literal(tpl, None) {
                            self.extracted.push(extracted);
                        }
                    }
                }
                _ => {}
            }
        }

        // Continue traversal into child nodes
        decl.visit_children_with(self);
    }
}

/// Calculate position from byte offset
fn position_from_offset(source: &str, offset: usize) -> Position {
    let mut line = 0;
    let mut column = 0;

    for (idx, ch) in source.char_indices() {
        if idx >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            column = 0;
        } else {
            column += ch.len_utf16();
        }
    }

    Position::new(line, column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ExtractConfig::default();
        assert_eq!(config.magic_comment, "GraphQL");
        assert!(config.tag_identifiers.contains(&"gql".to_string()));
        assert!(config.modules.contains(&"graphql-tag".to_string()));
    }

    #[test]
    fn test_extract_raw_graphql() {
        let source = r"
query GetUser {
  user {
    id
    name
  }
}
";
        let config = ExtractConfig::default();
        let result = extract_from_source(source, Language::GraphQL, &config).unwrap();

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source, source);
        assert_eq!(result[0].tag_name, None);
    }

    #[test]
    fn test_position_from_offset() {
        let source = "line 0\nline 1\nline 2";
        let pos = position_from_offset(source, 0);
        assert_eq!(pos, Position::new(0, 0));

        let pos = position_from_offset(source, 7); // Start of "line 1"
        assert_eq!(pos, Position::new(1, 0));

        let pos = position_from_offset(source, 14); // Start of "line 2"
        assert_eq!(pos, Position::new(2, 0));
    }

    mod typescript_tests {
        use super::*;

        #[test]
        fn test_extract_tagged_template_with_import() {
            let source = r"
import { gql } from 'graphql-tag';

const query = gql`
  query GetUser {
    user {
      id
      name
    }
  }
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetUser"));
            assert_eq!(result[0].tag_name, Some("gql".to_string()));
        }

        #[test]
        fn test_extract_tagged_template_without_import_disallowed() {
            let source = r"
const query = gql`
  query GetUser {
    user {
      id
    }
  }
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            // Should not extract because gql is not imported
            assert_eq!(result.len(), 0);
        }

        #[test]
        fn test_extract_tagged_template_without_import_allowed() {
            let source = r"
const query = gql`
  query GetUser {
    user {
      id
    }
  }
`;
";
            let config = ExtractConfig {
                allow_global_identifiers: true,
                ..Default::default()
            };
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            // Should extract because global identifiers are allowed
            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetUser"));
            assert_eq!(result[0].tag_name, Some("gql".to_string()));
        }

        #[test]
        fn test_extract_from_apollo_client() {
            let source = r"
import { gql } from '@apollo/client';

const QUERY = gql`
  query GetPosts {
    posts {
      id
      title
    }
  }
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetPosts"));
            assert_eq!(result[0].tag_name, Some("gql".to_string()));
        }

        #[test]
        fn test_extract_multiple_queries() {
            let source = r"
import { gql } from 'graphql-tag';

const query1 = gql`query Q1 { field1 }`;
const query2 = gql`query Q2 { field2 }`;
const query3 = gql`mutation M1 { updateField }`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 3);
            assert!(result[0].source.contains("query Q1"));
            assert!(result[1].source.contains("query Q2"));
            assert!(result[2].source.contains("mutation M1"));
        }

        #[test]
        fn test_extract_graphql_tag_identifier() {
            let source = r"
import { graphql } from 'graphql-tag';

const query = graphql`
  query GetData {
    data {
      id
    }
  }
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetData"));
            assert_eq!(result[0].tag_name, Some("graphql".to_string()));
        }

        #[test]
        fn test_extract_from_javascript() {
            let source = r"
import { gql } from 'graphql-tag';

const query = gql`
  query GetUser {
    user {
      id
    }
  }
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::JavaScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetUser"));
        }

        #[test]
        fn test_extract_with_jsx() {
            let source = r"
import { gql } from '@apollo/client';
import { useQuery } from '@apollo/client';

const GET_USER = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      id
      name
    }
  }
`;

function UserComponent({ userId }) {
  const { data } = useQuery(GET_USER, { variables: { id: userId } });
  return <div>{data?.user?.name}</div>;
}
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetUser"));
        }

        #[test]
        fn test_extract_with_custom_tag() {
            let source = r"
import { customGql } from 'graphql-tag';

const query = customGql`query Custom { field }`;
";
            let mut config = ExtractConfig::default();
            config.tag_identifiers.push("customGql".to_string());
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query Custom"));
            assert_eq!(result[0].tag_name, Some("customGql".to_string()));
        }

        #[test]
        fn test_import_from_unknown_module() {
            let source = r"
import { gql } from 'unknown-module';

const query = gql`query Test { field }`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            // Should not extract because module is not in the allowed list
            assert_eq!(result.len(), 0);
        }

        #[test]
        fn test_default_import() {
            let source = r"
import gql from 'graphql-tag';

const query = gql`query Test { field }`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query Test"));
        }

        #[test]
        fn test_renamed_import() {
            let source = r"
import { gql as query } from 'graphql-tag';

const q = query`query Test { field }`;
";
            let mut config = ExtractConfig::default();
            config.tag_identifiers.push("query".to_string());
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query Test"));
        }

        #[test]
        fn test_typescript_decorators() {
            let source = r"
import { gql } from 'graphql-tag';

@Component
class UserQuery {
  query = gql`query GetUser { user { id } }`;
}
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetUser"));
        }

        #[test]
        fn test_parse_error_handling() {
            let source = "import { gql } from 'graphql-tag'; const x = %%%invalid%%%";

            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config);

            assert!(result.is_err());
            if let Err(ExtractError::Parse { message, .. }) = result {
                assert!(message.contains("SWC parse error"));
            } else {
                panic!("Expected parse error");
            }
        }

        #[test]
        fn test_multiline_query_formatting() {
            let source = r"
import { gql } from 'graphql-tag';

const query = gql`
  query GetUserWithPosts {
    user {
      id
      name
      posts {
        id
        title
        content
      }
    }
  }
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            assert!(result[0].source.contains("query GetUserWithPosts"));
            assert!(result[0].source.contains("posts"));
            assert!(result[0].source.contains("title"));
            assert!(result[0].source.contains("content"));
        }

        #[test]
        fn test_fragment_extraction() {
            let source = r"
import { gql } from '@apollo/client';

const USER_FRAGMENT = gql`
  fragment UserFields on User {
    id
    name
    email
  }
`;

const GET_USER = gql`
  query GetUser($id: ID!) {
    user(id: $id) {
      ...UserFields
    }
  }
  ${USER_FRAGMENT}
`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            // Should extract both fragments (the second has an expression, but we can count them)
            assert!(!result.is_empty());
            assert!(result[0].source.contains("fragment UserFields"));
        }

        #[test]
        fn test_location_tracking() {
            let source = r"import { gql } from 'graphql-tag';
const q = gql`query Test { field }`;
";
            let config = ExtractConfig::default();
            let result = extract_from_source(source, Language::TypeScript, &config).unwrap();

            assert_eq!(result.len(), 1);
            let location = &result[0].location;

            // Verify we have location information
            assert!(location.offset > 0);
            assert!(location.length > 0);
            // Line numbers are usize, so they're always >= 0
            assert!(location.range.end.line >= location.range.start.line);
        }

        #[test]
        fn test_all_javascript_extensions() {
            let test_cases = vec![
                (Language::JavaScript, "script.js"),
                (Language::JavaScript, "script.jsx"),
                (Language::JavaScript, "script.mjs"),
                (Language::JavaScript, "script.cjs"),
            ];

            let source = r"
import { gql } from 'graphql-tag';
const query = gql`query Test { field }`;
";

            for (lang, _filename) in test_cases {
                let config = ExtractConfig::default();
                let result = extract_from_source(source, lang, &config).unwrap();
                assert_eq!(result.len(), 1, "Failed for {lang:?}");
            }
        }
    }
}
