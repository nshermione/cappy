#[test]
fn compile_errors_are_reported_clearly() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/compile_fail/*.rs");
}
