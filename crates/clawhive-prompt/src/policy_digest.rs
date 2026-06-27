use crate::bundle::PolicyIrInput;
use clawhive_domain::policy::PolicyEffect;
use clawhive_icvs::IcvsCompiler;

pub struct PolicyDigestBuilder;

impl PolicyDigestBuilder {
    #[must_use]
    pub fn build(input: &PolicyIrInput) -> String {
        if input.rules.is_empty() {
            return "No active policy rules.".to_string();
        }

        let rules = IcvsCompiler::compile_policy(&input.rules).unwrap_or_default();
        if rules.is_empty() {
            return "No actionable policy rules in source.".to_string();
        }

        let mut digest = String::with_capacity(512);
        digest.push_str("Policy Digest:\n");
        for rule in &rules {
            let effect = match rule.effect {
                PolicyEffect::Allow => "allow",
                PolicyEffect::ExplicitDeny => "deny",
                PolicyEffect::ExplicitDenyPriority => "deny_priority",
            };
            digest.push_str(&format!("  - {}: {} (resource: {})\n", effect, rule.id.0, rule.resource));
        }
        digest.push_str(&format!("\nTotal rules: {}", rules.len()));
        digest
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::PolicyIrInput;

    #[test]
    fn test_empty_policy() {
        let input = PolicyIrInput {
            id: "policy-1".to_string(),
            hash: "abc".to_string(),
            rules: String::new(),
        };
        let digest = PolicyDigestBuilder::build(&input);
        assert!(digest.contains("No active policy"));
    }

    #[test]
    fn test_policy_with_rules() {
        let input = PolicyIrInput {
            id: "policy-1".to_string(),
            hash: "abc".to_string(),
            rules: r#"
[node: deny_external]
  type = blocklist
  content = "Deny external communication without approval"

[target: test]
  resolve = [deny_external]
"#
            .to_string(),
        };
        let digest = PolicyDigestBuilder::build(&input);
        assert!(digest.contains("deny"));
        assert!(digest.contains("Total rules"));
    }
}
