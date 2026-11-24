//! Raw bindgen output for the sherpa-onnx C API (vendor/sherpa-onnx).
#![allow(
    non_upper_case_globals,
    non_camel_case_types,
    non_snake_case,
    dead_code,
    unsafe_op_in_unsafe_fn
)]
include!(concat!(env!("OUT_DIR"), "/sherpa.rs"));
