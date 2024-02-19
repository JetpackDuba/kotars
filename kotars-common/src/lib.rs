use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub ty: JniType,
}

#[derive(Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<JniType>,
}

#[derive(Serialize, Deserialize)]
pub struct RsStruct {
    pub name: String,
    pub fields: Vec<(Option<String>, JniType)>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JniType {
    Int32,
    String,
    Boolean,
    CustomType(String),
}

impl From<String> for JniType {
    fn from(value: String) -> Self {
        match value.as_str() {
            "i32" => JniType::Int32,
            "String" => JniType::String,
            "bool" => JniType::Boolean,
            _ => JniType::CustomType(value.to_string()),
        }
    }
}