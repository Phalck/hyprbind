mod discover;
mod model;
mod parser;

pub use discover::discover;
pub use model::{Shortcut, Variable};
pub use parser::parse_file;
