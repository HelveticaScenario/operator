use modular_core::dsp::schema;

fn main() {
    let out = std::env::args()
        .nth(1)
        .expect("usage: generate-schemas <output-path>");
    let schemas = schema();
    let json = serde_json::to_string_pretty(&schemas).expect("failed to serialize schemas");
    std::fs::write(&out, json).expect("failed to write schemas file");
}
