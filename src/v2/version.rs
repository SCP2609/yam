//! Single source of truth for SemVer parsing and constraint matching.
//!
//! Both the local resolver (`core.rs`) and the registry resolver
//! (`registry.rs`) use this module instead of maintaining their own
//! string-based version comparisons, which previously disagreed with
//! each other and, in `core.rs`'s own hand-rolled `Version`, sorted a
//! pre-release *above* its release (`derive(Ord)` on
//! `pre: Option<String>` puts `None` below `Some(_)`).

use anyhow::{bail, Context, Result};
use semver::{Version, VersionReq};

/// Parse a package or artifact version. Package/artifact versions must be
/// a full `major.minor.patch` SemVer string; pre-release and build
/// metadata are allowed but never selected implicitly by a bare name.
pub fn parse_version(value: &str) -> Result<Version> {
    Version::parse(value).with_context(|| format!("version must be SemVer: {value:?}"))
}

/// Validate a version string without keeping the parsed value.
pub fn validate_version(value: &str) -> Result<()> {
    parse_version(value)?;
    Ok(())
}

/// A set of ANDed version constraints, such as `>=1.2 <2.0`.
#[derive(Debug, Clone)]
pub struct Constraints(VersionReq);

impl Constraints {
    /// Empty constraints: matches every non-pre-release version.
    pub fn any() -> Self {
        Self(VersionReq::STAR)
    }

    /// Parse space-separated constraints, each starting with one of
    /// `=`, `>=`, `<`, `^`, `~`. An empty string means "any version".
    pub fn parse(value: &str) -> Result<Self> {
        let words: Vec<&str> = value.split_whitespace().collect();
        if words.is_empty() {
            return Ok(Self::any());
        }
        for word in &words {
            if !["=", ">=", "<", "^", "~"]
                .iter()
                .any(|operator| word.starts_with(operator))
            {
                bail!("invalid version constraint {word:?}")
            }
        }
        let joined = words.join(",");
        let requirement = VersionReq::parse(&joined)
            .with_context(|| format!("invalid version constraint {value:?}"))?;
        Ok(Self(requirement))
    }

    pub fn matches(&self, version: &Version) -> bool {
        self.0.matches(version)
    }
}

/// Pick the highest version that is not a pre-release.
pub fn select_default<'a, I>(versions: I) -> Option<&'a Version>
where
    I: IntoIterator<Item = &'a Version>,
{
    versions
        .into_iter()
        .filter(|version| version.pre.is_empty())
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(value: &str) -> Version {
        parse_version(value).unwrap()
    }

    #[test]
    fn caret_follows_npm_semantics() {
        let c = Constraints::parse("^1.2").unwrap();
        assert!(c.matches(&v("1.9.0")));
        assert!(!c.matches(&v("2.0.0")));

        let c = Constraints::parse("^0.2").unwrap();
        assert!(c.matches(&v("0.2.5")));
        assert!(!c.matches(&v("0.3.0")));
    }

    #[test]
    fn tilde_pins_minor() {
        let c = Constraints::parse("~1.2.3").unwrap();
        assert!(c.matches(&v("1.2.9")));
        assert!(!c.matches(&v("1.3.0")));
    }

    #[test]
    fn compound_constraints_and_together() {
        let c = Constraints::parse(">=1 <2").unwrap();
        assert!(c.matches(&v("1.0.0")));
        assert!(c.matches(&v("1.9.9")));
        assert!(!c.matches(&v("2.0.0")));
        assert!(!c.matches(&v("0.9.0")));
    }

    #[test]
    fn a_prerelease_sorts_below_its_own_release() {
        // The bug this module fixes: a release must outrank its own
        // pre-releases, not the other way around.
        assert!(v("1.5.0-rc.1") < v("1.5.0"));
    }

    #[test]
    fn prerelease_is_never_selected_implicitly() {
        let candidates = [v("1.0.0"), v("1.1.0-rc.1"), v("0.9.0")];
        assert_eq!(select_default(candidates.iter()), Some(&v("1.0.0")));

        let c = Constraints::parse(">=1").unwrap();
        assert!(!c.matches(&v("1.1.0-rc.1")));
    }

    #[test]
    fn leading_zero_components_are_rejected() {
        assert!(validate_version("1.02.0").is_err());
        assert!(validate_version("1.2").is_err());
        assert!(validate_version("1.2.3").is_ok());
    }
}
