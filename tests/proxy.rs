// This file acts as the root for the 'proxy' integration test crate.
// It declares the modules containing the actual tests, which reside
// in the tests/proxy/ directory.

// Use the `proxy` subdirectory as a module
mod proxy {
    // Test modules in the proxy/ subdirectory
    mod connection_handler_tests;
    mod error_helpers_tests;
    mod events_tests;
    mod http_handlers_tests;
    mod http_tests;
    mod server_manager_tests;
    mod sse_proxy_tests;
    mod types_tests;
}
