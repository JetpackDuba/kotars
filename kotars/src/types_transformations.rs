use quote::quote;
use syn::__private::TokenStream2;
use kotars_common::JniType;

pub fn transform_jni_type_to_rust(
    jni_type: &JniType,
    param_name: &str,
) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => transform_jint_to_i32(param_name),
        JniType::Int64 => transform_jint_to_i32(param_name),
        JniType::String => transform_jstring_to_string(param_name),
        JniType::Boolean => transform_jbool_to_bool(param_name),
        JniType::CustomType(_) => transform_jobject_to_custom(param_name),
        JniType::Receiver(ty) => transform_jlong_to_receiver(param_name, ty),
    }
}

fn transform_jlong_to_receiver(param_name: &str, ty: &str) -> TokenStream2 {
    let param: TokenStream2 = syn::parse_str(param_name).unwrap();
    let ty: TokenStream2 = syn::parse_str(ty).unwrap();

    quote! {
        let #param = unsafe { &mut *(#param as *mut #ty) };
    }
}

pub fn transform_rust_to_jni_type(jni_type: &JniType, param_name: &str) -> TokenStream2 {
    match jni_type {
        JniType::Int32 => transform_i32_to_jint(param_name),
        JniType::Int64 => transform_i64_to_jlong(param_name),
        JniType::String => transform_string_to_jstring(param_name),
        JniType::Boolean => transform_bool_to_jbool(param_name),
        JniType::CustomType(_) => transform_custom_to_jobject(param_name),
        JniType::Receiver(_) => todo!(),
    }
}

fn transform_jint_to_i32(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { i32 })
}

fn transform_jint_to_i64(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { i64 })
}

fn transform_i32_to_jint(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jint })
}


fn transform_i64_to_jlong(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jlong })
}

fn transform_bool_to_jbool(param_name: &str) -> TokenStream2 {
    transform_types(param_name, quote! { jni::sys::jboolean })
}

fn transform_custom_to_jobject(param_name: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param = #param.into_env(&mut env); }
}

fn transform_jobject_to_custom(param_name: &str) -> TokenStream2 {
    let param = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param = #param.into_env(&mut env); }
}

fn transform_jbool_to_bool(param_name: &str) -> TokenStream2 {
    let transform = transform_types(param_name, quote! { u8 });
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        #transform
        let #param_name = #param_name == 1;
    }
}

fn transform_types(param_name: &str, target_type: TokenStream2) -> TokenStream2 {
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! { let #param_name = #param_name as #target_type; }
}

fn transform_jstring_to_string(param_name: &str) -> TokenStream2 {
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        let #param_name: String = env
            .get_string(&#param_name)
            .expect("Couldn't get java string!")
            .into();
    }
}

fn transform_string_to_jstring(param_name: &str) -> TokenStream2 {
    let param_name = syn::parse_str::<TokenStream2>(param_name).unwrap();
    quote! {
        let #param_name = env
            .new_string(#param_name)
            .expect("Couldn't create java string!");
    }
}