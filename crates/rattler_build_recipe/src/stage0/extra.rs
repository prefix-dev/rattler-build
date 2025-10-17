use std::fmt::Display;

use itertools::Itertools as _;
use serde::{Deserialize, Serialize};

use crate::stage0::types::ConditionalList;

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
pub struct Extra {
    #[serde(rename = "recipe-maintainers")]
    pub recipe_maintainers: ConditionalList<String>,
}

impl Display for Extra {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{{ recipe_maintainers: {} }}",
            self.recipe_maintainers.iter().format(", ")
        )
    }
}

impl Extra {
    pub fn used_variables(&self) -> Vec<String> {
        let mut vars = Vec::new();
        for maintainer in &self.recipe_maintainers {
            vars.extend(maintainer.used_variables());
        }
        vars.sort();
        vars.dedup();
        vars
    }
}
