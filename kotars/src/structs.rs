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
                fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv<'local>>) -> jni::objects::JObject<'local> {
                    let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");

                    let class_path = if package_name_for_signature.is_empty() {
                        format!("{}", #struct_name)
                    } else {
                        format!("{}/{}", package_name_for_signature, #struct_name)
                    };
                    let error_msg_new_object = format!("New object failed {class_path}");

                    let pointer = Box::into_raw(Box::new(self)) as jni::sys::jlong;
                    let constructor_signature = #constructor_signature.replace("<PKG_NAME>/", package_name_for_signature.as_str());

                    let error_msg = format!("Find class failed for {class_path}");
                    let class = env.find_class(class_path).expect(error_msg.as_str());

                    let constructor_args: &[jni::objects::JValue] = &[pointer.into()];
                    let obj = env.new_object(class, constructor_signature.as_str(), constructor_args).expect(&error_msg_new_object);
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
                fn into_env(self, env: &mut std::cell::RefMut<'_, jni::JNIEnv<'local>>) -> jni::objects::JObject<'local> {
                    let package_name_for_signature = crate::JNI_PACKAGE_NAME.replace(".", "/");
                    let rc_env = std::rc::Rc::new(std::cell::RefCell::new(env));
                    
                    let class_path = if package_name_for_signature.is_empty() {
                        format!("{}", #struct_name)
                    } else {
                        format!("{}/{}", package_name_for_signature, #struct_name)
                    };

                    let constructor_signature = #constructor_signature.replace(#PKG_NAME, package_name_for_signature.as_str());

                    let class = {
                        let mut env = rc_env.borrow_mut();
                        let error_msg = format!("Could not find class {class_path}");
                        env.find_class(class_path).expect(&error_msg)
                    };
                    
                    #(#transformations)*

                    let constructor_args: &[jni::objects::JValue] = &[#(#params_into_array,)*]; //vec![s.into()];
                    
                    let obj = {
                        let mut env = rc_env.borrow_mut();
                        env.new_object(class, constructor_signature.as_str(), constructor_args).unwrap()
                    };
                    
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
        JniType::UInt64 => "J".to_string(), // TODO This should be unsigned, perhaps use an object?
        JniType::Float32 => "F".to_string(),
        JniType::Float64 => "D".to_string(),
        JniType::String => "Ljava/lang/String;".to_string(),
        JniType::Boolean => "Z".to_string(),
        JniType::ByteArray => "[B".to_string(),
        JniType::CustomType(name) | JniType::Interface(name) => {
            // TODO At some point restore supporting package names format!("L{PKG_NAME}/{name};")
            format!("L{name};")
        }
        JniType::Void => "V".to_string(),
        JniType::Vec(ty) => {
            let inner_ty = jni_type_to_jni_method_signature_type(ty);
            format!("[{inner_ty}")
        },
        JniType::Option(ty) => jni_type_to_jni_method_signature_type(ty),
    }
}

fn generate_field_mapping_into_array(ty: &JniType, param: &TokenStream2) -> TokenStream2 {
    match ty {
        JniType::Int32 | JniType::Int64 | JniType::Float32 | JniType::Float64 | JniType::Boolean => {
            quote! { #param.into() }
        }
        JniType::UInt64 => {
            quote! { #param.into() } // TODO This should be unsigned, perhaps use an object?
        }
        JniType::Vec(_) | JniType::CustomType(_) | JniType::Interface(_) | JniType::ByteArray | JniType::String => {
            quote! { #param }
        }
        JniType::Receiver(_) => panic!("Structs can not have self as type"),
        JniType::Void => panic!("Structs can not have Void as type"),
        JniType::Option(ty) => generate_field_mapping_into_array(ty, param),
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
