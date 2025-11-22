use std::path::Path;

/// Supported source languages for GraphQL extraction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// Raw GraphQL files (.graphql, .gql, .gqls)
    GraphQL,
    /// TypeScript (.ts, .tsx)
    TypeScript,
    /// JavaScript (.js, .jsx, .mjs, .cjs)
    JavaScript,
    /// Vue Single File Components (.vue)
    Vue,
    /// Svelte components (.svelte)
    Svelte,
    /// Astro components (.astro)
    Astro,
}

impl Language {
    /// Detect language from file extension
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;

        match extension {
            "graphql" | "gql" | "gqls" => Some(Self::GraphQL),
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            "vue" => Some(Self::Vue),
            "svelte" => Some(Self::Svelte),
            "astro" => Some(Self::Astro),
            _ => None,
        }
    }

    /// Check if this language requires parsing (vs raw GraphQL)
    #[must_use]
    pub const fn requires_parsing(&self) -> bool {
        !matches!(self, Self::GraphQL)
    }

    /// Check if this language is TypeScript/JavaScript
    #[must_use]
    pub const fn is_js_family(&self) -> bool {
        matches!(self, Self::TypeScript | Self::JavaScript)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_language_from_path() {
        assert_eq!(
            Language::from_path(&PathBuf::from("schema.graphql")),
            Some(Language::GraphQL)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("query.gql")),
            Some(Language::GraphQL)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("component.ts")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("component.tsx")),
            Some(Language::TypeScript)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("script.js")),
            Some(Language::JavaScript)
        );
        assert_eq!(
            Language::from_path(&PathBuf::from("component.vue")),
            Some(Language::Vue)
        );
        assert_eq!(Language::from_path(&PathBuf::from("README.md")), None);
    }

    #[test]
    fn test_requires_parsing() {
        assert!(!Language::GraphQL.requires_parsing());
        assert!(Language::TypeScript.requires_parsing());
        assert!(Language::JavaScript.requires_parsing());
    }

    #[test]
    fn test_is_js_family() {
        assert!(Language::TypeScript.is_js_family());
        assert!(Language::JavaScript.is_js_family());
        assert!(!Language::GraphQL.is_js_family());
        assert!(!Language::Vue.is_js_family());
    }
}
