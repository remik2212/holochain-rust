use crate::wasm_engine::runtime::Runtime;
use holochain_core_types::error::HolochainError;
use std::sync::Arc;
use wasmer_runtime::{func, imports, instantiate, Instance, Module};
// use crate::nucleus::actions::trace_return_wasm_api_function::trace_return_wasm_api_function;
// use crate::nucleus::actions::trace_invoke_wasm_api_function::trace_invoke_wasm_api_function;
// use crate::nucleus::WasmApiFnCall;
// use std::convert::TryFrom;
use holochain_json_api::json::JsonString;
use std::convert::TryInto;
// use holochain_core_types::error::RibosomeEncodingBits;
use crate::wasm_engine::api::debug::invoke_debug;
use holochain_wasm_utils::memory::allocation::{Length, Offset, WasmAllocation};
use wasmer_runtime::error::RuntimeError;
// use holochain_core_types::error::RibosomeEncodingBits;
// use crate::wasm_engine::api::ZomeApiResult;
// use holochain_core_types::error::RibosomeEncodingBits;
use crate::wasm_engine::api::ZomeApiResult;
use std::sync::mpsc::channel;
use wasmer_runtime::Ctx;

/// Creates a WASM module, that is the executable program, from a given WASM binary byte array.
pub fn wasm_module_factory(wasm: Arc<Vec<u8>>) -> Result<Module, HolochainError> {
    let import_object = imports! {};
    Ok(instantiate(&wasm, &import_object)
        .map_err(|e| HolochainError::from(e.to_string()))?
        .module())
}

// fn invoke<J: TryFrom<JsonString>>(runtime: &Runtime, f: dyn (Fn(&Runtime, J) -> RibosomeEncodingBits), ptr: u32, len: u32) -> RibosomeEncodingBits {
//     match WasmAllocation::new(Offset::from(ptr), Length::from(len)) {
//         Ok(allocation) => {
//             let encoded_args = allocation.as_ribosome_encoding();
//             let parameters = runtime.load_json_string_from_args(encoded_args);
//             if let Ok(context) = runtime.context() {
//                 if let WasmCallData::ZomeCall(zome_call_data) = runtime.data.clone() {
//                     let zome_api_call = zome_call_data.call;
//                     let wasm_api_fn_call = WasmApiFnCall { function: self.clone(), parameters: parameters.clone() };
//                     trace_invoke_wasm_api_function(zome_api_call.clone(), wasm_api_fn_call.clone(), &context);
//                     let result = f(runtime, parameters.try_into()?);
//                     let wasm_api_fn_result = Ok(JsonString::from("TODO"));
//                     trace_return_wasm_api_function(zome_api_call.clone(), wasm_api_fn_call, wasm_api_fn_result, &context);
//                     result
//                 } else {
//                     error!("Can't record zome call wasm_api invocations for non zome call");
//                     f(runtime, parameters.try_into()?)
//                 }
//             } else {
//                 error!("Could not get context for runtime");
//                 f(runtime, parameters.try_into()?)
//             }
//         },
//         Err(allocation_error) => allocation_error.as_ribosome_encoding(),
//     }
// }

fn try_parameters_json(runtime: &Runtime, ptr: u32, len: u32) -> Result<JsonString, RuntimeError> {
    match WasmAllocation::new(Offset::from(ptr), Length::from(len)) {
        Ok(allocation) => {
            let encoded_args = allocation.as_ribosome_encoding();
            Ok(runtime.load_json_string_from_args(encoded_args))
        }
        Err(allocation_error) => Err(RuntimeError::Trap {
            msg: String::from(allocation_error).into_boxed_str(),
        }),
    }
}

/// Creates a runnable WASM module instance from a module reference.
/// Adds the Holochain specific API functions as imports.
pub fn wasm_instance_factory(
    runtime: &mut Runtime,
    module: &Module,
) -> Result<Instance, HolochainError> {
    let (sender, receiver) = channel();
    let import_object = imports! {
        "env" => {
            "hc_debug" => func!(move |ctx: &mut Ctx, ptr: u32, len: u32| -> ZomeApiResult {
                invoke_debug(runtime, try_parameters_json(runtime, ptr, len)?.try_into()?)
            }),
        },
    };
    Ok(module
        .instantiate(&import_object)
        .map_err(|e| HolochainError::from(e.to_string()))?)
}
