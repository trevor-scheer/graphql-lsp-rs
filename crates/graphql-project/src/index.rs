use std::collections::HashMap;

/// Index of schema types and fields for fast lookups
#[derive(Debug, Default)]
pub struct SchemaIndex {
    /// Type definitions (name -> definition)
    pub types: HashMap<String, TypeDefinition>,

    /// Field definitions by type
    pub fields: HashMap<String, Vec<FieldDefinition>>,
}

#[derive(Debug, Clone)]
pub struct TypeDefinition {
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

#[derive(Debug, Clone)]
pub struct FieldDefinition {
    pub name: String,
    pub type_name: String,
    pub description: Option<String>,
}

impl SchemaIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build index from schema string
    #[must_use]
    pub fn from_schema(_schema: &str) -> Self {
        // TODO: Parse schema with apollo-parser and build index
        Self::new()
    }

    /// Get a type by name
    #[must_use]
    pub fn get_type(&self, name: &str) -> Option<&TypeDefinition> {
        self.types.get(name)
    }

    /// Get fields for a type
    #[must_use]
    pub fn get_fields(&self, type_name: &str) -> Option<&[FieldDefinition]> {
        self.fields.get(type_name).map(Vec::as_slice)
    }
}

/// Index of GraphQL documents (operations and fragments)
#[derive(Debug, Default)]
pub struct DocumentIndex {
    /// Operation definitions (name -> location)
    pub operations: HashMap<String, OperationInfo>,

    /// Fragment definitions (name -> location)
    pub fragments: HashMap<String, FragmentInfo>,
}

#[derive(Debug, Clone)]
pub struct OperationInfo {
    pub name: Option<String>,
    pub operation_type: OperationType,
    pub file_path: String,
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
}

impl DocumentIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an operation to the index
    pub fn add_operation(&mut self, name: Option<String>, info: OperationInfo) {
        if let Some(name) = name {
            self.operations.insert(name, info);
        }
    }

    /// Add a fragment to the index
    pub fn add_fragment(&mut self, name: String, info: FragmentInfo) {
        self.fragments.insert(name, info);
    }

    /// Get an operation by name
    #[must_use]
    pub fn get_operation(&self, name: &str) -> Option<&OperationInfo> {
        self.operations.get(name)
    }

    /// Get a fragment by name
    #[must_use]
    pub fn get_fragment(&self, name: &str) -> Option<&FragmentInfo> {
        self.fragments.get(name)
    }
}
