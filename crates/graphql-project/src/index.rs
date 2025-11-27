use apollo_compiler::{
    schema::{ExtendedType, FieldDefinition},
    Schema,
};
use std::sync::Arc;

/// Index of schema types and fields for fast lookups
///
/// This is a lightweight wrapper around apollo-compiler's Schema that provides
/// convenient access patterns for LSP operations.
#[derive(Debug, Clone)]
pub struct SchemaIndex {
    schema: Arc<Schema>,
}

impl Default for SchemaIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaIndex {
    #[must_use]
    pub fn new() -> Self {
        // Create an empty schema
        let schema = Schema::parse("", "schema.graphql").unwrap_or_else(|_| {
            Schema::parse("type Query { _: String }", "schema.graphql")
                .expect("fallback schema should parse")
        });
        Self {
            schema: Arc::new(schema),
        }
    }

    /// Build index from multiple schema files using apollo-compiler
    ///
    /// Uses `SchemaBuilder` to parse each file separately, preserving source locations.
    ///
    /// # Returns
    ///
    /// A `SchemaIndex` with the parsed schema, or an empty schema if parsing fails.
    #[must_use]
    pub fn from_schema_files(schema_files: Vec<(String, String)>) -> Self {
        use apollo_compiler::schema::SchemaBuilder;

        if schema_files.is_empty() {
            return Self::new();
        }

        let mut builder = SchemaBuilder::new();

        // Parse each file separately so apollo-compiler tracks sources correctly
        for (path, content) in schema_files {
            builder = builder.parse(content, path);
        }

        // Build the schema
        match builder.build() {
            Ok(schema) => Self {
                schema: Arc::new(schema),
            },
            Err(diagnostics) => {
                tracing::warn!("Failed to build schema: {:?}", diagnostics);
                Self::new()
            }
        }
    }

    /// Build index from schema string using apollo-compiler
    ///
    /// Parses and validates the GraphQL schema using apollo-compiler,
    /// which provides comprehensive schema validation and query capabilities.
    ///
    /// # Returns
    ///
    /// A `SchemaIndex` with the parsed schema, or an empty schema if parsing fails.
    #[must_use]
    pub fn from_schema(schema_str: &str) -> Self {
        Self::from_schema_files(vec![("schema.graphql".to_string(), schema_str.to_string())])
    }

    /// Get the underlying apollo-compiler Schema
    #[must_use]
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// Get a type by name
    #[must_use]
    pub fn get_type(&self, name: &str) -> Option<TypeInfo> {
        let ext_type = self.schema.types.get(name)?;
        Some(TypeInfo::from_extended_type(ext_type))
    }

    /// Get fields for a type
    #[must_use]
    pub fn get_fields(&self, type_name: &str) -> Option<Vec<FieldInfo>> {
        let ext_type = self.schema.types.get(type_name)?;

        match ext_type {
            ExtendedType::Object(obj) => Some(
                obj.fields
                    .iter()
                    .map(|(_, field)| FieldInfo::from_field_definition(field))
                    .collect(),
            ),
            ExtendedType::Interface(iface) => Some(
                iface
                    .fields
                    .iter()
                    .map(|(_, field)| FieldInfo::from_field_definition(field))
                    .collect(),
            ),
            ExtendedType::InputObject(input) => Some(
                input
                    .fields
                    .iter()
                    .map(|(_, input_field)| FieldInfo {
                        name: input_field.name.to_string(),
                        type_name: input_field.ty.to_string(),
                        description: input_field
                            .description
                            .as_ref()
                            .map(std::string::ToString::to_string),
                        deprecated: None, // Input fields don't have deprecation
                        arguments: Vec::new(),
                    })
                    .collect(),
            ),
            _ => None,
        }
    }

    /// Get a directive by name
    #[must_use]
    pub fn get_directive(&self, name: &str) -> Option<DirectiveInfo> {
        let directive = self.schema.directive_definitions.get(name)?;
        Some(DirectiveInfo {
            name: directive.name.to_string(),
            description: directive
                .description
                .as_ref()
                .map(std::string::ToString::to_string),
            locations: directive
                .locations
                .iter()
                .map(|loc| format!("{loc:?}"))
                .collect(),
        })
    }

    /// Find the location of a field definition in the schema source
    ///
    /// Returns the line, column (0-indexed), and file path where the field is defined
    /// in the schema source using apollo-compiler's built-in location tracking.
    #[must_use]
    pub fn find_field_definition(
        &self,
        type_name: &str,
        field_name: &str,
    ) -> Option<FieldDefinitionLocation> {
        use apollo_compiler::schema::ExtendedType;

        // Get the type from the schema
        let extended_type = self.schema.types.get(type_name)?;

        // Extract fields based on type kind
        let fields = match extended_type {
            ExtendedType::Object(obj) => &obj.fields,
            ExtendedType::Interface(iface) => &iface.fields,
            _ => return None,
        };

        // Find the field
        let field_component = fields.get(field_name)?;
        let field_node = &field_component.node;

        // Get the location from the Node
        let location = field_node.location()?;

        // Convert to line/column using the schema's source map
        let line_col_range = field_node.line_column_range(&self.schema.sources)?;

        tracing::info!(
            "Apollo compiler line_col_range for {}.{}: start.line={}, start.column={}",
            type_name,
            field_name,
            line_col_range.start.line,
            line_col_range.start.column
        );

        // Get the file path from the source map
        let file_id = location.file_id();
        let file_path = self
            .schema
            .sources
            .get(&file_id)?
            .path()
            .to_string_lossy()
            .to_string();

        let result_line = line_col_range.start.line.saturating_sub(1);
        let result_col = line_col_range.start.column.saturating_sub(1);

        tracing::info!(
            "After converting to 0-indexed: line={}, col={}",
            result_line,
            result_col
        );

        Some(FieldDefinitionLocation {
            line: result_line,  // Convert to 0-indexed
            column: result_col, // Convert to 0-indexed
            field_name: field_name.to_string(),
            file_path,
        })
    }
}

/// Location information for a field definition in schema
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldDefinitionLocation {
    pub line: usize,
    pub column: usize,
    pub field_name: String,
    pub file_path: String,
}

/// Type information extracted from schema
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub name: String,
    pub kind: TypeKind,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeKind {
    Object,
    Interface,
    Union,
    Enum,
    InputObject,
    Scalar,
}

impl TypeInfo {
    fn from_extended_type(ext_type: &ExtendedType) -> Self {
        let (kind, description) = match ext_type {
            ExtendedType::Object(obj) => (
                TypeKind::Object,
                obj.description
                    .as_ref()
                    .map(std::string::ToString::to_string),
            ),
            ExtendedType::Interface(iface) => (
                TypeKind::Interface,
                iface
                    .description
                    .as_ref()
                    .map(std::string::ToString::to_string),
            ),
            ExtendedType::Union(union) => (
                TypeKind::Union,
                union
                    .description
                    .as_ref()
                    .map(std::string::ToString::to_string),
            ),
            ExtendedType::Enum(enum_def) => (
                TypeKind::Enum,
                enum_def
                    .description
                    .as_ref()
                    .map(std::string::ToString::to_string),
            ),
            ExtendedType::InputObject(input) => (
                TypeKind::InputObject,
                input
                    .description
                    .as_ref()
                    .map(std::string::ToString::to_string),
            ),
            ExtendedType::Scalar(scalar) => (
                TypeKind::Scalar,
                scalar
                    .description
                    .as_ref()
                    .map(std::string::ToString::to_string),
            ),
        };

        Self {
            name: ext_type.name().to_string(),
            kind,
            description,
        }
    }
}

/// Field information extracted from schema
#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub name: String,
    pub type_name: String,
    pub description: Option<String>,
    pub deprecated: Option<String>,
    pub arguments: Vec<ArgumentInfo>,
}

impl FieldInfo {
    fn from_field_definition(field: &FieldDefinition) -> Self {
        // Check if the field has a @deprecated directive
        let deprecated = field.directives.get("deprecated").and_then(|directive| {
            // Try to get the "reason" argument from the directive
            // The directive has arguments stored as a Vec of Argument nodes
            directive
                .arguments
                .iter()
                .find(|arg| arg.name.as_str() == "reason")
                .and_then(|arg| {
                    // Extract string value from the argument
                    // The value is a Node<apollo_compiler::ast::Value>
                    if let apollo_compiler::ast::Value::String(reason_str) = arg.value.as_ref() {
                        Some(reason_str.clone())
                    } else {
                        None
                    }
                })
                .or_else(|| Some("No longer supported".to_string()))
        });

        let arguments = field
            .arguments
            .iter()
            .map(|arg| ArgumentInfo {
                name: arg.name.to_string(),
                type_name: arg.ty.to_string(),
                description: arg
                    .description
                    .as_ref()
                    .map(std::string::ToString::to_string),
                default_value: arg.default_value.as_ref().map(ToString::to_string),
            })
            .collect();

        Self {
            name: field.name.to_string(),
            type_name: field.ty.to_string(),
            description: field
                .description
                .as_ref()
                .map(std::string::ToString::to_string),
            deprecated,
            arguments,
        }
    }
}

/// Argument information extracted from field definitions
#[derive(Debug, Clone)]
pub struct ArgumentInfo {
    pub name: String,
    pub type_name: String,
    pub description: Option<String>,
    pub default_value: Option<String>,
}

/// Directive information extracted from schema
#[derive(Debug, Clone)]
pub struct DirectiveInfo {
    pub name: String,
    pub description: Option<String>,
    pub locations: Vec<String>,
}

/// Index of GraphQL documents (operations and fragments)
#[derive(Debug, Default)]
pub struct DocumentIndex {
    /// Operation definitions (name -> locations)
    /// Changed to Vec to track all occurrences for duplicate detection
    pub operations: std::collections::HashMap<String, Vec<OperationInfo>>,

    /// Fragment definitions (name -> locations)
    /// Changed to Vec to track all occurrences for duplicate detection
    pub fragments: std::collections::HashMap<String, Vec<FragmentInfo>>,
}

#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub name: Option<String>,
    pub operation_type: OperationType,
    pub file_path: String,
    /// Line number (0-indexed) where the operation name appears
    pub line: usize,
    /// Column number (0-indexed) where the operation name appears
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationType {
    Query,
    Mutation,
    Subscription,
}

#[derive(Debug, Clone)]
pub struct FragmentInfo {
    pub name: String,
    pub type_condition: String,
    pub file_path: String,
    /// Line number (0-indexed) where the fragment name appears
    pub line: usize,
    /// Column number (0-indexed) where the fragment name appears
    pub column: usize,
}

impl DocumentIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an operation to the index
    pub fn add_operation(&mut self, name: Option<String>, info: OperationInfo) {
        if let Some(name) = name {
            self.operations.entry(name).or_default().push(info);
        }
    }

    /// Add a fragment to the index
    pub fn add_fragment(&mut self, name: String, info: FragmentInfo) {
        self.fragments.entry(name).or_default().push(info);
    }

    /// Get operations by name (returns all occurrences)
    #[must_use]
    pub fn get_operations(&self, name: &str) -> Option<&Vec<OperationInfo>> {
        self.operations.get(name)
    }

    /// Get the first operation by name (for backward compatibility)
    #[must_use]
    pub fn get_operation(&self, name: &str) -> Option<&OperationInfo> {
        self.operations.get(name).and_then(|ops| ops.first())
    }

    /// Get fragments by name (returns all occurrences)
    #[must_use]
    pub fn get_fragments_by_name(&self, name: &str) -> Option<&Vec<FragmentInfo>> {
        self.fragments.get(name)
    }

    /// Get the first fragment by name (for backward compatibility)
    #[must_use]
    pub fn get_fragment(&self, name: &str) -> Option<&FragmentInfo> {
        self.fragments.get(name).and_then(|frags| frags.first())
    }

    /// Check for duplicate operation and fragment names across the project
    ///
    /// Returns a list of diagnostics for any duplicate names found, with one diagnostic
    /// per occurrence at the actual file location
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn check_duplicate_names(&self) -> Vec<(String, crate::Diagnostic)> {
        use crate::{Diagnostic, Position, Range};
        let mut diagnostics = Vec::new();

        // Check for duplicate operation names
        for (name, operations) in &self.operations {
            if operations.len() > 1 {
                for op in operations {
                    let message = format!(
                        "Operation name '{}' is not unique across the project. Found {} definitions.",
                        name,
                        operations.len()
                    );

                    // Use the actual position from the operation
                    let range = Range {
                        start: Position {
                            line: op.line,
                            character: op.column,
                        },
                        end: Position {
                            line: op.line,
                            character: op.column + name.len(),
                        },
                    };

                    let mut diag = Diagnostic::error(range, message)
                        .with_code("unique-operation-names-project")
                        .with_source("graphql-validator");

                    // Add related information for all other occurrences
                    for other_op in operations {
                        if other_op.file_path != op.file_path
                            || other_op.line != op.line
                            || other_op.column != op.column
                        {
                            use crate::diagnostics::{Location, RelatedInfo};
                            let other_range = Range {
                                start: Position {
                                    line: other_op.line,
                                    character: other_op.column,
                                },
                                end: Position {
                                    line: other_op.line,
                                    character: other_op.column + name.len(),
                                },
                            };
                            let related = RelatedInfo {
                                message: format!("Operation '{name}' also defined here"),
                                location: Location {
                                    uri: format!("file://{}", other_op.file_path),
                                    range: other_range,
                                },
                            };
                            diag = diag.with_related_info(related);
                        }
                    }

                    diagnostics.push((op.file_path.clone(), diag));
                }
            }
        }

        // Check for duplicate fragment names
        for (name, fragments) in &self.fragments {
            if fragments.len() > 1 {
                for frag in fragments {
                    let message = format!(
                        "Fragment name '{}' is not unique across the project. Found {} definitions.",
                        name,
                        fragments.len()
                    );

                    // Use the actual position from the fragment
                    let range = Range {
                        start: Position {
                            line: frag.line,
                            character: frag.column,
                        },
                        end: Position {
                            line: frag.line,
                            character: frag.column + name.len(),
                        },
                    };

                    let mut diag = Diagnostic::error(range, message)
                        .with_code("unique-fragment-names-project")
                        .with_source("graphql-validator");

                    // Add related information for all other occurrences
                    for other_frag in fragments {
                        if other_frag.file_path != frag.file_path
                            || other_frag.line != frag.line
                            || other_frag.column != frag.column
                        {
                            use crate::diagnostics::{Location, RelatedInfo};
                            let other_range = Range {
                                start: Position {
                                    line: other_frag.line,
                                    character: other_frag.column,
                                },
                                end: Position {
                                    line: other_frag.line,
                                    character: other_frag.column + name.len(),
                                },
                            };
                            let related = RelatedInfo {
                                message: format!("Fragment '{name}' also defined here"),
                                location: Location {
                                    uri: format!("file://{}", other_frag.file_path),
                                    range: other_range,
                                },
                            };
                            diag = diag.with_related_info(related);
                        }
                    }

                    diagnostics.push((frag.file_path.clone(), diag));
                }
            }
        }

        diagnostics
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_object_type() {
        let schema = r"
            type User {
                id: ID!
                name: String!
                email: String
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        // Check type exists
        let user_type = index.get_type("User").expect("User type should exist");
        assert_eq!(user_type.name, "User");
        assert_eq!(user_type.kind, TypeKind::Object);

        // Check fields
        let fields = index.get_fields("User").expect("User fields should exist");
        assert_eq!(fields.len(), 3);

        assert_eq!(fields[0].name, "id");
        assert_eq!(fields[0].type_name, "ID!");

        assert_eq!(fields[1].name, "name");
        assert_eq!(fields[1].type_name, "String!");

        assert_eq!(fields[2].name, "email");
        assert_eq!(fields[2].type_name, "String");
    }

    #[test]
    fn test_parse_interface_type() {
        let schema = r"
            interface Node {
                id: ID!
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        let node_type = index.get_type("Node").expect("Node type should exist");
        assert_eq!(node_type.kind, TypeKind::Interface);

        let fields = index.get_fields("Node").expect("Node fields should exist");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "id");
    }

    #[test]
    fn test_parse_union_type() {
        let schema = r"
            type User { id: ID! }
            type Post { id: ID! }
            type Comment { id: ID! }
            union SearchResult = User | Post | Comment
        ";

        let index = SchemaIndex::from_schema(schema);

        let union_type = index
            .get_type("SearchResult")
            .expect("SearchResult type should exist");
        assert_eq!(union_type.kind, TypeKind::Union);
    }

    #[test]
    fn test_parse_enum_type() {
        let schema = r"
            enum Status {
                ACTIVE
                INACTIVE
                PENDING
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        let enum_type = index.get_type("Status").expect("Status type should exist");
        assert_eq!(enum_type.kind, TypeKind::Enum);
    }

    #[test]
    fn test_parse_scalar_type() {
        let schema = r"
            scalar DateTime
        ";

        let index = SchemaIndex::from_schema(schema);

        let scalar_type = index
            .get_type("DateTime")
            .expect("DateTime type should exist");
        assert_eq!(scalar_type.kind, TypeKind::Scalar);
    }

    #[test]
    fn test_parse_input_object_type() {
        let schema = r"
            input CreateUserInput {
                name: String!
                email: String!
                age: Int
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        let input_type = index
            .get_type("CreateUserInput")
            .expect("CreateUserInput type should exist");
        assert_eq!(input_type.kind, TypeKind::InputObject);

        let fields = index
            .get_fields("CreateUserInput")
            .expect("CreateUserInput fields should exist");
        assert_eq!(fields.len(), 3);
    }

    #[test]
    fn test_parse_field_with_arguments() {
        let schema = r"
            type User { id: ID! }
            type Query {
                user(id: ID!): User
                users(limit: Int = 10, offset: Int): [User!]!
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        let fields = index
            .get_fields("Query")
            .expect("Query fields should exist");
        assert_eq!(fields.len(), 2);

        // Test user field with single argument
        assert_eq!(fields[0].name, "user");
        assert_eq!(fields[0].arguments.len(), 1);
        assert_eq!(fields[0].arguments[0].name, "id");
        assert_eq!(fields[0].arguments[0].type_name, "ID!");

        // Test users field with multiple arguments and default value
        assert_eq!(fields[1].name, "users");
        assert_eq!(fields[1].arguments.len(), 2);
        assert_eq!(fields[1].arguments[0].name, "limit");
        assert_eq!(fields[1].arguments[0].type_name, "Int");
        assert!(fields[1].arguments[0].default_value.is_some());
        assert_eq!(fields[1].arguments[1].name, "offset");
        assert_eq!(fields[1].arguments[1].type_name, "Int");
    }

    #[test]
    fn test_parse_list_types() {
        let schema = r"
            type Comment { id: ID! }
            type Post {
                tags: [String!]!
                comments: [Comment]
                matrix: [[Int]]
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        let fields = index.get_fields("Post").expect("Post fields should exist");
        assert_eq!(fields.len(), 3);

        assert_eq!(fields[0].name, "tags");
        assert_eq!(fields[0].type_name, "[String!]!");

        assert_eq!(fields[1].name, "comments");
        assert_eq!(fields[1].type_name, "[Comment]");

        assert_eq!(fields[2].name, "matrix");
        assert_eq!(fields[2].type_name, "[[Int]]");
    }

    #[test]
    fn test_parse_with_descriptions() {
        let schema = r#"
            "Represents a user in the system"
            type User {
                "The unique identifier"
                id: ID!

                "The user's full name"
                name: String!
            }
        "#;

        let index = SchemaIndex::from_schema(schema);

        let user_type = index.get_type("User").expect("User type should exist");
        assert_eq!(
            user_type.description,
            Some("Represents a user in the system".to_string())
        );

        let fields = index.get_fields("User").expect("User fields should exist");
        assert_eq!(
            fields[0].description,
            Some("The unique identifier".to_string())
        );
        assert_eq!(
            fields[1].description,
            Some("The user's full name".to_string())
        );
    }

    #[test]
    fn test_parse_block_string_descriptions() {
        let schema = r#"
            """
            A user account in the system.
            Can be either active or inactive.
            """
            type User {
                id: ID!
            }
        "#;

        let index = SchemaIndex::from_schema(schema);

        let user_type = index.get_type("User").expect("User type should exist");
        assert!(user_type.description.is_some());
        let desc = user_type.description.as_ref().unwrap();
        assert!(desc.contains("user account"));
    }

    #[test]
    fn test_parse_deprecated_directive() {
        let schema = r#"
            type User {
                id: ID!
                username: String! @deprecated(reason: "Use 'name' instead")
                name: String!
                oldField: Int @deprecated
            }
        "#;

        let index = SchemaIndex::from_schema(schema);

        let fields = index.get_fields("User").expect("User fields should exist");

        assert!(fields[0].deprecated.is_none()); // id
                                                 // apollo-compiler marks the field as deprecated but doesn't expose reason easily
        assert!(fields[1].deprecated.is_some()); // username
        assert!(fields[2].deprecated.is_none()); // name
        assert!(fields[3].deprecated.is_some()); // oldField
    }

    #[test]
    fn test_parse_directive_definition() {
        let schema = r"
            directive @auth(
                requires: String!
            ) on FIELD_DEFINITION | OBJECT
        ";

        let index = SchemaIndex::from_schema(schema);

        let directive = index
            .get_directive("auth")
            .expect("auth directive should exist");
        assert_eq!(directive.name, "auth");
        assert_eq!(directive.locations.len(), 2);
    }

    #[test]
    fn test_parse_multiple_types() {
        let schema = r"
            type User {
                id: ID!
                name: String!
            }

            type Post {
                id: ID!
                title: String!
                author: User!
            }

            enum PostStatus {
                DRAFT
                PUBLISHED
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        assert!(index.get_type("User").is_some());
        assert!(index.get_type("Post").is_some());
        assert!(index.get_type("PostStatus").is_some());
    }

    #[test]
    fn test_parse_complex_github_like_schema() {
        let schema = r#"
            """
            Represents a user account
            """
            type User implements Node {
                "Unique identifier"
                id: ID!

                "User's login name"
                login: String!

                "User's display name"
                name: String

                "User's email address"
                email: String!

                "User's repositories"
                repositories(
                    first: Int = 10,
                    after: String,
                    orderBy: RepositoryOrder
                ): RepositoryConnection!
            }

            interface Node {
                id: ID!
            }

            type RepositoryConnection {
                edges: [RepositoryEdge!]!
                pageInfo: PageInfo!
            }

            type RepositoryEdge {
                node: Repository
                cursor: String!
            }

            type Repository {
                id: ID!
                name: String!
                description: String
                stars: Int!
            }

            type PageInfo {
                hasNextPage: Boolean!
                endCursor: String
            }

            input RepositoryOrder {
                field: RepositoryOrderField!
                direction: OrderDirection!
            }

            enum RepositoryOrderField {
                CREATED_AT
                UPDATED_AT
                NAME
            }

            enum OrderDirection {
                ASC
                DESC
            }
        "#;

        let index = SchemaIndex::from_schema(schema);

        // Check that all types are indexed
        assert!(index.get_type("User").is_some());
        assert!(index.get_type("Node").is_some());
        assert!(index.get_type("Repository").is_some());
        assert!(index.get_type("RepositoryConnection").is_some());
        assert!(index.get_type("RepositoryOrder").is_some());
        assert!(index.get_type("RepositoryOrderField").is_some());

        // Check User fields
        let user_fields = index.get_fields("User").expect("User fields should exist");
        assert_eq!(user_fields.len(), 5);

        // Check repositories field with arguments
        let repos_field = user_fields
            .iter()
            .find(|f| f.name == "repositories")
            .expect("repositories field should exist");
        assert_eq!(repos_field.arguments.len(), 3);
        assert_eq!(repos_field.type_name, "RepositoryConnection!");

        // Verify input object fields
        let order_fields = index
            .get_fields("RepositoryOrder")
            .expect("RepositoryOrder fields should exist");
        assert_eq!(order_fields.len(), 2);
    }

    #[test]
    fn test_parse_shopify_like_schema() {
        let schema = r"
            type Product {
                id: ID!
                title: String!
                description: String
                price: Money!
                variants(first: Int = 10): ProductVariantConnection!
                tags: [String!]!
            }

            type Money {
                amount: String!
                currencyCode: String!
            }

            type ProductVariant {
                id: ID!
                title: String!
                price: Money!
                availableForSale: Boolean!
            }

            type ProductVariantConnection {
                edges: [ProductVariantEdge!]!
            }

            type ProductVariantEdge {
                node: ProductVariant!
            }

            input ProductFilter {
                available: Boolean
                productType: String
                tags: [String!]
            }

            type Query {
                product(id: ID!): Product
                products(
                    first: Int = 10,
                    filter: ProductFilter
                ): [Product!]!
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        // Check Product type
        let product = index.get_type("Product").expect("Product should exist");
        assert_eq!(product.kind, TypeKind::Object);

        let product_fields = index
            .get_fields("Product")
            .expect("Product fields should exist");
        assert_eq!(product_fields.len(), 6);

        // Check variants field has arguments
        let variants_field = product_fields
            .iter()
            .find(|f| f.name == "variants")
            .expect("variants field should exist");
        assert_eq!(variants_field.arguments.len(), 1);
        assert!(variants_field.arguments[0].default_value.is_some());

        // Check Query type
        let query_fields = index
            .get_fields("Query")
            .expect("Query fields should exist");
        assert_eq!(query_fields.len(), 2);

        let products_field = query_fields
            .iter()
            .find(|f| f.name == "products")
            .expect("products field should exist");
        assert_eq!(products_field.arguments.len(), 2);
    }

    #[test]
    fn test_error_handling_invalid_schema() {
        let schema = r"
            type User {
                id: ID!
                name: String!
                // Missing closing brace
        ";

        let index = SchemaIndex::from_schema(schema);

        // Should return empty schema on syntax errors
        assert!(index.get_type("User").is_none());
    }

    #[test]
    fn test_empty_schema() {
        let schema = "";

        let index = SchemaIndex::from_schema(schema);

        // Empty schema won't have User type
        assert!(index.get_type("User").is_none());
    }

    #[test]
    fn test_type_without_fields() {
        let schema = r"
            type Empty
        ";

        let index = SchemaIndex::from_schema(schema);

        let empty_type = index.get_type("Empty").expect("Empty type should exist");
        assert_eq!(empty_type.kind, TypeKind::Object);

        // Should have no fields
        let fields = index.get_fields("Empty");
        assert!(fields.is_none() || fields.unwrap().is_empty());
    }

    #[test]
    fn test_argument_with_description() {
        let schema = r#"
            type Result { id: ID! }
            type Query {
                search(
                    "The search query string"
                    query: String!

                    "Maximum number of results"
                    limit: Int = 20
                ): [Result!]!
            }
        "#;

        let index = SchemaIndex::from_schema(schema);

        let fields = index
            .get_fields("Query")
            .expect("Query fields should exist");
        let search_field = &fields[0];

        assert_eq!(search_field.arguments.len(), 2);
        assert_eq!(
            search_field.arguments[0].description,
            Some("The search query string".to_string())
        );
        assert_eq!(
            search_field.arguments[1].description,
            Some("Maximum number of results".to_string())
        );
    }

    #[test]
    fn test_document_index_tracks_duplicate_operations() {
        let mut index = DocumentIndex::new();

        // Add two operations with the same name
        index.add_operation(
            Some("GetUser".to_string()),
            OperationInfo {
                name: Some("GetUser".to_string()),
                operation_type: OperationType::Query,
                file_path: "/path/to/file1.graphql".to_string(),
                line: 1,
                column: 6,
            },
        );

        index.add_operation(
            Some("GetUser".to_string()),
            OperationInfo {
                name: Some("GetUser".to_string()),
                operation_type: OperationType::Query,
                file_path: "/path/to/file2.graphql".to_string(),
                line: 5,
                column: 6,
            },
        );

        // Verify both operations are tracked
        let operations = index
            .get_operations("GetUser")
            .expect("Should have operations");
        assert_eq!(operations.len(), 2);

        // Verify duplicate detection works
        let diagnostics = index.check_duplicate_names();
        assert_eq!(
            diagnostics.len(),
            2,
            "Should have 2 errors (one for each occurrence)"
        );
        assert!(diagnostics
            .iter()
            .all(|(_, d)| d.message.contains("GetUser")));
        assert!(diagnostics
            .iter()
            .all(|(_, d)| d.message.contains("not unique across the project")));

        // Verify file paths are correct
        assert_eq!(diagnostics[0].0, "/path/to/file1.graphql");
        assert_eq!(diagnostics[1].0, "/path/to/file2.graphql");
    }

    #[test]
    fn test_document_index_tracks_duplicate_fragments() {
        let mut index = DocumentIndex::new();

        // Add two fragments with the same name
        index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file1.graphql".to_string(),
                line: 2,
                column: 9,
            },
        );

        index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file2.graphql".to_string(),
                line: 10,
                column: 9,
            },
        );

        // Verify both fragments are tracked
        let fragments = index
            .get_fragments_by_name("UserFields")
            .expect("Should have fragments");
        assert_eq!(fragments.len(), 2);

        // Verify duplicate detection works
        let diagnostics = index.check_duplicate_names();
        assert_eq!(
            diagnostics.len(),
            2,
            "Should have 2 errors (one for each occurrence)"
        );
        assert!(diagnostics
            .iter()
            .all(|(_, d)| d.message.contains("UserFields")));
        assert!(diagnostics
            .iter()
            .all(|(_, d)| d.message.contains("not unique across the project")));
    }

    #[test]
    fn test_document_index_unique_names_no_errors() {
        let mut index = DocumentIndex::new();

        // Add unique operations
        index.add_operation(
            Some("GetUser".to_string()),
            OperationInfo {
                name: Some("GetUser".to_string()),
                operation_type: OperationType::Query,
                file_path: "/path/to/file1.graphql".to_string(),
                line: 0,
                column: 6,
            },
        );

        index.add_operation(
            Some("GetUsers".to_string()),
            OperationInfo {
                name: Some("GetUsers".to_string()),
                operation_type: OperationType::Query,
                file_path: "/path/to/file2.graphql".to_string(),
                line: 0,
                column: 6,
            },
        );

        // Add unique fragments
        index.add_fragment(
            "UserFields".to_string(),
            FragmentInfo {
                name: "UserFields".to_string(),
                type_condition: "User".to_string(),
                file_path: "/path/to/file1.graphql".to_string(),
                line: 5,
                column: 9,
            },
        );

        index.add_fragment(
            "PostFields".to_string(),
            FragmentInfo {
                name: "PostFields".to_string(),
                type_condition: "Post".to_string(),
                file_path: "/path/to/file2.graphql".to_string(),
                line: 10,
                column: 9,
            },
        );

        // Verify no duplicates
        let diagnostics = index.check_duplicate_names();
        assert_eq!(
            diagnostics.len(),
            0,
            "Should have no errors for unique names"
        );
    }
}
