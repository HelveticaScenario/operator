//! Category grouping for module schemas.
//!
//! The DSL splits schemas across `crates/modular/dsl/src/generated/factories/<category>.ts`
//! so the generated files stay small. Each category here corresponds to a
//! `crates/modular_core/src/dsp/<category>/` module.

use crate::dsp;
use crate::types::ModuleSchema;

/// One DSL factory category.
pub struct Category {
    pub name: &'static str,
    pub schemas: Vec<ModuleSchema>,
}

/// All categories in deterministic order. Mirrors the order in `dsp::schema()`.
pub fn all_categories() -> Vec<Category> {
    vec![
        Category {
            name: "core",
            schemas: dsp::core::schemas(),
        },
        Category {
            name: "dynamics",
            schemas: dsp::dynamics::schemas(),
        },
        Category {
            name: "fx",
            schemas: dsp::fx::schemas(),
        },
        Category {
            name: "oscillators",
            schemas: dsp::oscillators::schemas(),
        },
        Category {
            name: "filters",
            schemas: dsp::filters::schemas(),
        },
        Category {
            name: "phase",
            schemas: dsp::phase::schemas(),
        },
        Category {
            name: "utilities",
            schemas: dsp::utilities::schemas(),
        },
        Category {
            name: "seq",
            schemas: dsp::seq::schemas(),
        },
        Category {
            name: "midi",
            schemas: dsp::midi::schemas(),
        },
        Category {
            name: "samplers",
            schemas: dsp::samplers::schemas(),
        },
    ]
}
