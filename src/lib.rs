mod fingerprint_gen;
mod hanning;
mod signature;

// use fingerprint_gen:
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn generate_fingerprint() -> String {
    String::from("Hello from rust!")
}

// pub fn add(left: usize, right: usize) -> usize {
//     left + right
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }
