#![allow(
	clippy::dbg_macro,
	clippy::expect_used,
	clippy::missing_docs_in_private_items,
	clippy::print_stderr,
	clippy::print_stdout,
	clippy::unwrap_used
)]

use project_name::hello_world;

#[test]
fn it_says_hello() {
	hello_world();
}
