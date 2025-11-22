use apollo_parser::{cst, cst::CstNode, Parser};
use std::collections::HashMap;

/// Index of schema types and fields for fast lookups
///
/// This index provides O(1) lookup for type definitions and their fields,
/// which is essential for LSP features like autocomplete and go-to-definition.
#[derive(Debug, Default)]
pub struct SchemaIndex {
    /// Type definitions (name -> definition)
    pub types: HashMap<String, TypeDefinition>,

    /// Field definitions by type
    pub fields: HashMap<String, Vec<FieldDefinition>>,

    /// Directive definitions
    pub directives: HashMap<String, DirectiveDefinition>,
}

#[derive(Debug, Clone)]
pub struct TypeDefinition {
    pub name: String,
    pub kind: TypeKind,
    pub description: Option<String>,
    pub deprecated: Option<String>,
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
    pub deprecated: Option<String>,
    pub arguments: Vec<ArgumentDefinition>,
}

#[derive(Debug, Clone)]
pub struct ArgumentDefinition {
    pub name: String,
    pub type_name: String,
    pub description: Option<String>,
    pub default_value: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DirectiveDefinition {
    pub name: String,
    pub description: Option<String>,
    pub locations: Vec<String>,
}

impl SchemaIndex {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build index from schema string using apollo-parser
    ///
    /// Parses the GraphQL schema and builds an efficient index for:
    /// - Type definitions (object, interface, union, enum, scalar, input)
    /// - Field definitions with their types and arguments
    /// - Directive definitions
    ///
    /// # Returns
    ///
    /// A populated `SchemaIndex` ready for O(1) lookups, or an empty index
    /// if the schema contains syntax errors.
    #[must_use]
    pub fn from_schema(schema: &str) -> Self {
        let parser = Parser::new(schema);
        let tree = parser.parse();

        // If there are syntax errors, return empty index
        // TODO: Convert errors to diagnostics for LSP reporting
        if tree.errors().len() > 0 {
            return Self::default();
        }

        let document = tree.document();
        let mut index = Self::default();

        // Process all definitions in the schema
        for definition in document.definitions() {
            match definition {
                cst::Definition::ObjectTypeDefinition(obj) => {
                    index.process_object_type(&obj);
                }
                cst::Definition::InterfaceTypeDefinition(interface) => {
                    index.process_interface_type(&interface);
                }
                cst::Definition::UnionTypeDefinition(union) => {
                    index.process_union_type(&union);
                }
                cst::Definition::EnumTypeDefinition(enum_def) => {
                    index.process_enum_type(&enum_def);
                }
                cst::Definition::InputObjectTypeDefinition(input) => {
                    index.process_input_object_type(&input);
                }
                cst::Definition::ScalarTypeDefinition(scalar) => {
                    index.process_scalar_type(&scalar);
                }
                cst::Definition::DirectiveDefinition(directive) => {
                    index.process_directive_definition(&directive);
                }
                // Extensions can be processed later if needed
                // Operations and fragments are not part of schema definitions
                // Schema definition is less critical for LSP features
                cst::Definition::ObjectTypeExtension(_)
                | cst::Definition::InterfaceTypeExtension(_)
                | cst::Definition::UnionTypeExtension(_)
                | cst::Definition::EnumTypeExtension(_)
                | cst::Definition::InputObjectTypeExtension(_)
                | cst::Definition::ScalarTypeExtension(_)
                | cst::Definition::SchemaDefinition(_)
                | cst::Definition::SchemaExtension(_)
                | cst::Definition::OperationDefinition(_)
                | cst::Definition::FragmentDefinition(_) => {
                    // TODO: Handle type extensions
                }
            }
        }

        index
    }

    /// Process an object type definition
    fn process_object_type(&mut self, obj: &cst::ObjectTypeDefinition) {
        let Some(name_node) = obj.name() else {
            return;
        };
        let name = name_node.text().to_string();

        // Extract description from leading comments
        let description = obj
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));

        // Check for @deprecated directive
        let deprecated = extract_deprecated_directive(obj.directives().as_ref());

        // Add type definition
        self.types.insert(
            name.clone(),
            TypeDefinition {
                name: name.clone(),
                kind: TypeKind::Object,
                description,
                deprecated,
            },
        );

        // Index fields
        if let Some(fields_def) = obj.fields_definition() {
            let fields: Vec<FieldDefinition> = fields_def
                .field_definitions()
                .filter_map(|f| Self::process_field_definition(&f))
                .collect();

            if !fields.is_empty() {
                self.fields.insert(name, fields);
            }
        }
    }

    /// Process an interface type definition
    fn process_interface_type(&mut self, interface: &cst::InterfaceTypeDefinition) {
        let Some(name_node) = interface.name() else {
            return;
        };
        let name = name_node.text().to_string();

        let description = interface
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));
        let deprecated = extract_deprecated_directive(interface.directives().as_ref());

        self.types.insert(
            name.clone(),
            TypeDefinition {
                name: name.clone(),
                kind: TypeKind::Interface,
                description,
                deprecated,
            },
        );

        // Index fields
        if let Some(fields_def) = interface.fields_definition() {
            let fields: Vec<FieldDefinition> = fields_def
                .field_definitions()
                .filter_map(|f| Self::process_field_definition(&f))
                .collect();

            if !fields.is_empty() {
                self.fields.insert(name, fields);
            }
        }
    }

    /// Process a union type definition
    fn process_union_type(&mut self, union: &cst::UnionTypeDefinition) {
        let Some(name_node) = union.name() else {
            return;
        };
        let name = name_node.text().to_string();

        let description = union
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));
        let deprecated = extract_deprecated_directive(union.directives().as_ref());

        self.types.insert(
            name,
            TypeDefinition {
                name: name_node.text().to_string(),
                kind: TypeKind::Union,
                description,
                deprecated,
            },
        );
    }

    /// Process an enum type definition
    fn process_enum_type(&mut self, enum_def: &cst::EnumTypeDefinition) {
        let Some(name_node) = enum_def.name() else {
            return;
        };
        let name = name_node.text().to_string();

        let description = enum_def
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));
        let deprecated = extract_deprecated_directive(enum_def.directives().as_ref());

        self.types.insert(
            name,
            TypeDefinition {
                name: name_node.text().to_string(),
                kind: TypeKind::Enum,
                description,
                deprecated,
            },
        );
    }

    /// Process an input object type definition
    fn process_input_object_type(&mut self, input: &cst::InputObjectTypeDefinition) {
        let Some(name_node) = input.name() else {
            return;
        };
        let name = name_node.text().to_string();

        let description = input
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));
        let deprecated = extract_deprecated_directive(input.directives().as_ref());

        self.types.insert(
            name.clone(),
            TypeDefinition {
                name: name.clone(),
                kind: TypeKind::InputObject,
                description,
                deprecated,
            },
        );

        // Index input fields as regular fields for autocomplete
        if let Some(fields_def) = input.input_fields_definition() {
            let fields: Vec<FieldDefinition> = fields_def
                .input_value_definitions()
                .filter_map(|f| {
                    let name_node = f.name()?;
                    let type_node = f.ty()?;

                    Some(FieldDefinition {
                        name: name_node.text().to_string(),
                        type_name: extract_type_name(&type_node),
                        description: f
                            .description()
                            .and_then(|d| d.string_value())
                            .map(|sv| extract_string_value(&sv)),
                        deprecated: extract_deprecated_directive(f.directives().as_ref()),
                        arguments: Vec::new(),
                    })
                })
                .collect();

            if !fields.is_empty() {
                self.fields.insert(name, fields);
            }
        }
    }

    /// Process a scalar type definition
    fn process_scalar_type(&mut self, scalar: &cst::ScalarTypeDefinition) {
        let Some(name_node) = scalar.name() else {
            return;
        };
        let name = name_node.text().to_string();

        let description = scalar
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));
        let deprecated = extract_deprecated_directive(scalar.directives().as_ref());

        self.types.insert(
            name,
            TypeDefinition {
                name: name_node.text().to_string(),
                kind: TypeKind::Scalar,
                description,
                deprecated,
            },
        );
    }

    /// Process a directive definition
    fn process_directive_definition(&mut self, directive: &cst::DirectiveDefinition) {
        let Some(name_node) = directive.name() else {
            return;
        };
        let name = name_node.text().to_string();

        let description = directive
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));

        let locations = directive
            .directive_locations()
            .map(|locs| {
                locs.directive_locations()
                    .map(|loc| loc.source_string())
                    .collect()
            })
            .unwrap_or_default();

        self.directives.insert(
            name.clone(),
            DirectiveDefinition {
                name,
                description,
                locations,
            },
        );
    }

    /// Process a field definition and extract all metadata
    fn process_field_definition(field: &cst::FieldDefinition) -> Option<FieldDefinition> {
        let name_node = field.name()?;
        let type_node = field.ty()?;

        let description = field
            .description()
            .and_then(|d| d.string_value())
            .map(|sv| extract_string_value(&sv));
        let deprecated = extract_deprecated_directive(field.directives().as_ref());

        // Extract arguments
        let arguments = field
            .arguments_definition()
            .map(|args_def| {
                args_def
                    .input_value_definitions()
                    .filter_map(|arg| {
                        let arg_name = arg.name()?.text().to_string();
                        let arg_type = extract_type_name(&arg.ty()?);
                        let arg_description = arg
                            .description()
                            .and_then(|d| d.string_value())
                            .map(|sv| extract_string_value(&sv));
                        let default_value = arg
                            .default_value()
                            .and_then(|dv| dv.value())
                            .map(|v| v.source_string());

                        Some(ArgumentDefinition {
                            name: arg_name,
                            type_name: arg_type,
                            description: arg_description,
                            default_value,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Some(FieldDefinition {
            name: name_node.text().to_string(),
            type_name: extract_type_name(&type_node),
            description,
            deprecated,
            arguments,
        })
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

    /// Get a directive by name
    #[must_use]
    pub fn get_directive(&self, name: &str) -> Option<&DirectiveDefinition> {
        self.directives.get(name)
    }
}

/// Extract the type name from a Type CST node, handling lists and non-null modifiers
fn extract_type_name(ty: &cst::Type) -> String {
    match ty {
        cst::Type::NamedType(named) => named
            .name()
            .map_or_else(|| "Unknown".to_string(), |n| n.text().to_string()),
        cst::Type::ListType(list) => {
            let inner = list
                .ty()
                .map_or_else(|| "Unknown".to_string(), |t| extract_type_name(&t));
            format!("[{inner}]")
        }
        cst::Type::NonNullType(non_null) => {
            // Check if it wraps a list type first
            non_null.list_type().map_or_else(
                || {
                    non_null.named_type().map_or_else(
                        || "Unknown!".to_string(),
                        |named_type| {
                            let inner = named_type
                                .name()
                                .map_or_else(|| "Unknown".to_string(), |n| n.text().to_string());
                            format!("{inner}!")
                        },
                    )
                },
                |list_type| {
                    let inner = list_type
                        .ty()
                        .map_or_else(|| "Unknown".to_string(), |t| extract_type_name(&t));
                    format!("[{inner}]!")
                },
            )
        }
    }
}

/// Extract string value from a `StringValue` node, removing quotes
fn extract_string_value(string_value: &cst::StringValue) -> String {
    let text = string_value.source_string();
    // Remove leading/trailing quotes and handle block strings
    if text.starts_with("\"\"\"") && text.ends_with("\"\"\"") {
        text[3..text.len() - 3].trim().to_string()
    } else if text.starts_with('"') && text.ends_with('"') {
        text[1..text.len() - 1].to_string()
    } else {
        text
    }
}

/// Extract @deprecated directive reason from directives
fn extract_deprecated_directive(directives: Option<&cst::Directives>) -> Option<String> {
    let directives = directives?;

    for directive in directives.directives() {
        let name = directive.name()?;
        if name.text() == "deprecated" {
            // Try to extract reason argument
            if let Some(args) = directive.arguments() {
                for arg in args.arguments() {
                    if let Some(arg_name) = arg.name() {
                        if arg_name.text() == "reason" {
                            if let Some(cst::Value::StringValue(sv)) = arg.value() {
                                return Some(extract_string_value(&sv));
                            }
                        }
                    }
                }
            }
            // Default deprecated message if no reason provided
            return Some("No longer supported".to_string());
        }
    }

    None
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
        assert!(user_type.description.is_none());

        // Check fields
        let fields = index.get_fields("User").expect("User fields should exist");
        assert_eq!(fields.len(), 3);

        let id_field = &fields[0];
        assert_eq!(id_field.name, "id");
        assert_eq!(id_field.type_name, "ID!");

        let name_field = &fields[1];
        assert_eq!(name_field.name, "name");
        assert_eq!(name_field.type_name, "String!");

        let email_field = &fields[2];
        assert_eq!(email_field.name, "email");
        assert_eq!(email_field.type_name, "String");
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
            type Query {
                user(id: ID!): User
                users(limit: Int = 10, offset: Int): [User!]!
            }
        ";

        let index = SchemaIndex::from_schema(schema);

        let fields = index.get_fields("Query").expect("Query fields should exist");
        assert_eq!(fields.len(), 2);

        // Test user field with single argument
        let user_field = &fields[0];
        assert_eq!(user_field.name, "user");
        assert_eq!(user_field.arguments.len(), 1);
        assert_eq!(user_field.arguments[0].name, "id");
        assert_eq!(user_field.arguments[0].type_name, "ID!");

        // Test users field with multiple arguments and default value
        let list_field = &fields[1];
        assert_eq!(list_field.name, "users");
        assert_eq!(list_field.arguments.len(), 2);
        assert_eq!(list_field.arguments[0].name, "limit");
        assert_eq!(list_field.arguments[0].type_name, "Int");
        assert_eq!(list_field.arguments[0].default_value, Some("10".to_string()));
        assert_eq!(list_field.arguments[1].name, "offset");
        assert_eq!(list_field.arguments[1].type_name, "Int");
    }

    #[test]
    fn test_parse_list_types() {
        let schema = r"
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
        assert_eq!(fields[1].description, Some("The user's full name".to_string()));
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
        assert_eq!(
            fields[1].deprecated,
            Some("Use 'name' instead".to_string())
        ); // username
        assert!(fields[2].deprecated.is_none()); // name
        assert_eq!(
            fields[3].deprecated,
            Some("No longer supported".to_string())
        ); // oldField (no reason)
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
        assert!(directive.locations.contains(&"FIELD_DEFINITION".to_string()));
        assert!(directive.locations.contains(&"OBJECT".to_string()));
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

        assert_eq!(index.types.len(), 3);
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

        let product_fields = index.get_fields("Product").expect("Product fields should exist");
        assert_eq!(product_fields.len(), 6);

        // Check variants field has arguments
        let variants_field = product_fields
            .iter()
            .find(|f| f.name == "variants")
            .expect("variants field should exist");
        assert_eq!(variants_field.arguments.len(), 1);
        assert_eq!(variants_field.arguments[0].default_value, Some("10".to_string()));

        // Check Query type
        let query_fields = index.get_fields("Query").expect("Query fields should exist");
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

        // Should return empty index on syntax errors
        assert_eq!(index.types.len(), 0);
        assert_eq!(index.fields.len(), 0);
    }

    #[test]
    fn test_empty_schema() {
        let schema = "";

        let index = SchemaIndex::from_schema(schema);

        assert_eq!(index.types.len(), 0);
        assert_eq!(index.fields.len(), 0);
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
        assert!(index.get_fields("Empty").is_none());
    }

    #[test]
    fn test_argument_with_description() {
        let schema = r#"
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

        let fields = index.get_fields("Query").expect("Query fields should exist");
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
}
