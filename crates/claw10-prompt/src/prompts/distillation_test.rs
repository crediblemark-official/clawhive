use super::*;
use claw10_icvs::IcvsCompiler;

#[test]
fn test_distillation_compiles() {
    let prompts = IcvsCompiler::compile_prompt(DISTILLATION_SOURCE, "distillation").unwrap();
    assert_eq!(prompts.len(), 2);
    assert!(prompts[1].content.contains("subsistem memori"));
}
