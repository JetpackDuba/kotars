extern crate proc_macro;

use std::fmt::format;
use kotars_common::{Field, JniType, RsStruct};
use quote::quote;
use syn::__private::{str, TokenStream2};
use syn::{ItemStruct, Visibility};

use crate::{full_header_comment, rust_property_to_jni_type};

const PKG_NAME: &str = "<PKG_NAME>";

pub struct Class(RsStruct);

pub trait JniGenerator {
    fn generated_methods(&self) -> Vec<TokenStream2>;
}

impl From<RsStruct> for Class {
    fn from(value: RsStruct) -> Self {
        Class(value)
    }
}

pub struct DataClass(RsStruct);

impl From<RsStruct> for DataClass {
    fn from(value: RsStruct) -> Self {
        DataClass(value)
    }
}

impl JniGenerator for Class {
    fn generated_methods(&self) -> Vec<TokenStream2> {
        let map_to_class: TokenStream2 = self.map_to_class_func();
        let drop: TokenStream2 = self.drop_func();

        vec![map_to_class, drop]
    }
}

impl JniGenerator for DataClass {

    fn generated_methods(&self) -> Vec<TokenStream2> {
        let map_to_class: TokenStream2 = self.map_to_data_class_func();

        vec![map_to_class]
    }
}

impl Class {
    fn map_to_class_func(&self) -> TokenStream2 {
        let struct_name = &self.0.name;
        let struct_token: TokenStream2 = syn::parse_str(struct_name).unwrap();
        let constructor_types_signature = jni_type_to_jni_method_signature_type(&JniType::Int64);
        let constructor_signature = format!("({constructor_types_signature})V");
        let struct_json = serde_json::to_string(&self.0).unwrap();

        let header_param = format!("JNI_CLASS {struct_json}");
        let header_comments = full_header_comment(header_param.as_str());

        quote! {
            #header_comments
            impl <'local> crate::IntoEnv<'local, jni::objects::JObject<'local>> for #struct_token {
                fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JObject<'local> {
                    let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");

                    let class_path = if package_name_for_signature.is_empty() {
                        format!("{}", #struct_name)
                    } else {
                        format!("{}/{}", package_name_for_signature, #struct_name)
                    };

                    let pointer = Box::into_raw(Box::new(self)) as jni::sys::jlong;
                    let constructor_signature = #constructor_signature.replace(#PKG_NAME, package_name_for_signature.as_str());

                    let class = env.find_class(class_path).expect("Find class failed");

                    let constructor_args: &[jni::objects::JValue] = &[pointer.into()]; //vec![s.into()];
                    let obj = env.new_object(class, constructor_signature.as_str(), constructor_args).expect("New object failed");
                    obj
                }
            }
        }
    }

    fn drop_func(&self) -> TokenStream2 {
        let drop_func_header = format!("Java_{}Obj_destroy", self.0.name);
        let drop_func_header: TokenStream2 = syn::parse_str(&drop_func_header).unwrap();
        let struct_token: TokenStream2 = syn::parse_str(&self.0.name).unwrap();

        quote! {
            #[no_mangle]
            pub unsafe extern "system" fn #drop_func_header(
                _env: jni::JNIEnv,
                _class: jni::objects::JClass,
                jni_pointer: jni::sys::jlong,
            ) {
                drop(Box::from_raw(jni_pointer as *mut #struct_token))
            }
        }
    }
}

impl DataClass {
    fn map_to_data_class_func(&self) -> TokenStream2 {
        let struct_name = &self.0.name;
        let struct_token: TokenStream2 = syn::parse_str(struct_name).unwrap();

        let constructor_types_signature = self
            .0
            .fields
            .iter()
            .map(|field| jni_type_to_jni_method_signature_type(&field.ty))
            .collect::<Vec<String>>()
            .join("");

        let transformations = generate_struct_fields_transformation(&self.0.fields);
        let params_into_array = generate_struct_fields_mapping_into_array(&self.0.fields);
        let constructor_signature = format!("({constructor_types_signature})V");
        let struct_json = serde_json::to_string(&self.0).unwrap();
        
        let header_param = format!("JNI_DATA_CLASS {struct_json}");
        let header_comments = full_header_comment(header_param.as_str());

        quote! {
            #header_comments
            impl <'local> crate::IntoEnv<'local, jni::objects::JObject<'local>> for #struct_token {
                fn into_env(self, env: &mut jni::JNIEnv<'local>) -> jni::objects::JObject<'local> {
                    let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");

                    let class_path = if package_name_for_signature.is_empty() {
                        format!("{}", #struct_name)
                    } else {
                        format!("{}/{}", package_name_for_signature, #struct_name)
                    };

                    let constructor_signature = #constructor_signature.replace(#PKG_NAME, package_name_for_signature.as_str());

                    let class = env.find_class(class_path).unwrap();
                    #(#transformations)*

                    let constructor_args: &[jni::objects::JValue] = &[#(#params_into_array,)*]; //vec![s.into()];
                    let obj = env.new_object(class, constructor_signature.as_str(), constructor_args).unwrap();
                    obj
                }
            }
        }
    }
}

pub fn generate_struct_fields_transformation(fields: &[Field]) -> Vec<TokenStream2> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = &field.safe_name(&index);
            let ty = &field.ty;
            let param = syn::parse_str(name).unwrap();

            let struct_parameter = match field.name.as_ref() {
                None => quote! { self.#index },
                Some(_) => {
                    quote! { self.#param }
                }
            };

            rust_property_to_jni_type(ty, &param, &struct_parameter)
        })
        .collect::<Vec<TokenStream2>>()
}

pub fn generate_method_fields_transformation(fields: &[Field]) -> Vec<TokenStream2> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let name = &field.safe_name(&index);
            let ty = &field.ty;
            let param = syn::parse_str(name).unwrap_or_else(|_| panic!("Couldn't parse {name}"));

            let struct_parameter = match field.name.as_ref() {
                None => panic!("All interface function fields should be named."),
                Some(_) => {
                    quote! { #param }
                }
            };

            rust_property_to_jni_type(ty, &param, &struct_parameter)
        })
        .collect::<Vec<TokenStream2>>()
}

pub fn generate_struct_fields_mapping_into_array(fields: &[Field]) -> Vec<TokenStream2> {
    fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let param = syn::parse_str(&field.safe_name(&index))
                .expect("Unable to deserialize parse field name.");

            generate_field_mapping_into_array(&field.ty, &param)
        })
        .collect::<Vec<TokenStream2>>()
}

pub fn jni_type_to_jni_method_signature_type(jni_type: &JniType) -> String {
    match jni_type {
        JniType::Int32 => "I".to_string(),
        JniType::Int64 | JniType::Receiver(_) => "J".to_string(),
        JniType::String => "Ljava/lang/String;".to_string(),
        JniType::Boolean => "Z".to_string(),
        JniType::CustomType(name) | JniType::Interface(name) => {
            format!("L{PKG_NAME}/{name};")
        }
        JniType::Void => "V".to_string(),
    }
}

fn generate_field_mapping_into_array(ty: &JniType, param: &TokenStream2) -> TokenStream2 {
    match ty {
        JniType::Int32 => {
            quote! { #param.into() }
        }
        JniType::Int64 => {
            quote! { #param.into() }
        }
        JniType::String => {
            quote! { #param }
        }
        JniType::Boolean => {
            quote! { #param.into() }
        }
        JniType::CustomType(_) | JniType::Interface(_) => {
            quote! { #param }
        }
        JniType::Receiver(_) => panic!("Structs can not have self as type"),
        JniType::Void => panic!("Structs can not have Void as type"),
    }
}

pub trait FromSyn {
    fn from_syn(value: ItemStruct) -> Self;
}

impl FromSyn for RsStruct {
    fn from_syn(value: ItemStruct) -> Self {
        let fields = value
            .fields
            .iter()
            .map(|field| {
                let name = field.ident.as_ref().map(|id| quote! { #id }.to_string());
                let original_ty = &field.ty;
                let ty = quote! { #original_ty }.to_string();
                let jni_ty: JniType = ty.into();

                Field {
                    is_public: matches!(field.vis, Visibility::Public { .. }),
                    name,
                    ty: jni_ty,
                }
            })
            .collect::<Vec<Field>>();

        RsStruct {
            name: value.ident.to_string(),
            fields,
        }
    }
}
