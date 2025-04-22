mod parser;
pub mod validator;

pub use parser::{Config, ServerConfig};
pub use validator::validate_config;