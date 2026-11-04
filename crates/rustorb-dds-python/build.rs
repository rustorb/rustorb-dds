//! Build script: the link arguments of a Python extension module.

fn main() {
    // On macOS, an extension module leaves the Python symbols to be found
    // when it is loaded (maturin passes the same arguments).
    pyo3_build_config::add_extension_module_link_args();
}
