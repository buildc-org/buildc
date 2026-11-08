use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Debug)]
pub struct Version(String);

fn segment(part: &str) -> &str {
    part.trim_start_matches('0')
}

fn segment_cmp(left: &str, right: &str) -> Ordering {
    let (left, right) = (segment(left), segment(right));
    left.len().cmp(&right.len()).then_with(|| left.cmp(right))
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let dotted = !text.is_empty()
            && text
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
        dotted.then(|| Self(text.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let mut one = self.0.split('.');
        let mut other = other.0.split('.');
        loop {
            match (one.next(), other.next()) {
                (None, None) => return Ordering::Equal,
                (Some(left), Some(right)) => {
                    let found = segment_cmp(left, right);
                    if found != Ordering::Equal {
                        return found;
                    }
                }
                (Some(left), None) => {
                    let found = segment_cmp(left, "0");
                    if found != Ordering::Equal {
                        return found;
                    }
                }
                (None, Some(right)) => {
                    let found = segment_cmp("0", right);
                    if found != Ordering::Equal {
                        return found;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).expect("a dotted version")
    }

    #[test]
    fn a_trailing_zero_segment_does_not_change_the_version() {
        assert_eq!(version("1.44"), version("1.44.0"));
        assert_eq!(version("2025.3.0.0"), version("2025.3"));
    }

    #[test]
    fn a_longer_version_with_a_nonzero_extra_segment_is_greater() {
        assert!(version("2025.3.1") > version("2025.3"));
        assert!(version("1.44.1") > version("1.44"));
    }

    #[test]
    fn two_different_versions_are_not_equal() {
        assert_ne!(version("1.44"), version("1.45"));
        assert_ne!(version("2025.3"), version("2025.3.1"));
    }

    #[test]
    fn a_later_segment_decides_before_the_one_after_it() {
        assert!(version("2025.3") < version("2025.3.1"));
        assert!(version("2025.9") < version("2025.10"));
        assert!(version("1.0") < version("2"));
    }

    #[test]
    fn a_segment_too_large_for_a_machine_integer_still_compares() {
        assert!(
            version("1.999999999999999999999999999999")
                > version("1.100000000000000000000000000000")
        );
    }

    #[test]
    fn only_dotted_digits_are_a_version() {
        assert!(Version::parse("2025.3").is_some());
        assert!(Version::parse("1").is_some());
        for text in [
            "",
            "2025.",
            ".3",
            "2025..3",
            "2025.3-rc1",
            "v2025.3",
            " 1.0",
        ] {
            assert!(Version::parse(text).is_none(), "{text}");
        }
    }
}
