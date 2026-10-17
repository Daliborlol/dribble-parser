//! Converts `ExtrasOutput` to the JS object of section 2.5, group by group. Every Rust vector is
//! dropped right after its JS copy is made, so the Rust and JS copies of the whole output never
//! coexist.

use js_sys::{
    Array, Float32Array, Int16Array, Int32Array, Int8Array, Object, Reflect, Uint16Array,
    Uint32Array, Uint8Array,
};
use wasm_bindgen::JsValue;

use super::out::XNode;
use super::ExtrasOutput;

/// Converts one node, consuming it.
pub fn to_js(node: XNode) -> JsValue {
    match node {
        XNode::Obj(fields) => {
            let obj = Object::new();
            for (k, v) in fields {
                let value = to_js(v);
                let _ = Reflect::set(&obj, &JsValue::from_str(&k), &value);
            }
            obj.into()
        }
        XNode::Arr(items) => {
            let arr = Array::new();
            for v in items {
                arr.push(&to_js(v));
            }
            arr.into()
        }
        XNode::U8(v) => Uint8Array::from(&v[..]).into(),
        XNode::I8(v) => Int8Array::from(&v[..]).into(),
        XNode::U16(v) => Uint16Array::from(&v[..]).into(),
        XNode::I16(v) => Int16Array::from(&v[..]).into(),
        XNode::U32(v) => Uint32Array::from(&v[..]).into(),
        XNode::I32(v) => Int32Array::from(&v[..]).into(),
        XNode::F32(v) => Float32Array::from(&v[..]).into(),
        XNode::Num(n) => JsValue::from_f64(n),
        XNode::Bool(b) => JsValue::from_bool(b),
        XNode::Str(s) => JsValue::from_str(&s),
        XNode::Strs(list) => {
            let arr = Array::new();
            for s in list {
                arr.push(&JsValue::from_str(&s));
            }
            arr.into()
        }
        XNode::Events(list) => match serde_wasm_bindgen::to_value(&list) {
            Ok(v) => v,
            Err(_) => Array::new().into(),
        },
    }
}

/// The `ExtrasRaw` object.
#[inline(never)]
pub fn output_to_js(out: ExtrasOutput) -> JsValue {
    let ExtrasOutput { groups, .. } = out;
    let obj = Object::new();
    for (name, node) in groups {
        let value = to_js(node);
        let _ = Reflect::set(&obj, &JsValue::from_str(&name), &value);
    }
    obj.into()
}
