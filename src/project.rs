//! Projects: the container a person records into.

use serde::{Deserialize, Serialize};

/// Longest project or recording name accepted.
///
/// Shared so the website can refuse a name before the round trip and the
/// backend can refuse the same name after it, without the two disagreeing.
pub const MAX_NAME_LEN: usize = 120;

/// Why a name was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NameError {
    Empty,
    TooLong,
}

impl NameError {
    pub fn message(&self) -> &'static str {
        match self {
            Self::Empty => "A name is required.",
            Self::TooLong => "That name is too long.",
        }
    }
}

/// Trim a name and check it, returning the form that should be stored.
///
/// Returns the trimmed name so a caller cannot validate one string and store a
/// different one, which is how a name passes validation and then arrives with
/// trailing whitespace.
pub fn validate_name(name: &str) -> Result<String, NameError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(NameError::Empty);
    }
    // Counted in characters, not bytes, so a name of emoji or non-Latin script
    // is not rejected for being three times its apparent length.
    if trimmed.chars().count() > MAX_NAME_LEN {
        return Err(NameError::TooLong);
    }
    Ok(trimmed.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameProjectRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateProjectResponse {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeleteProjectResponse {
    pub message: String,
}

/// A project as the dashboard lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectResponse {
    pub id: String,
    pub name: String,
    pub recording_count: u32,
    /// Total length of every recording in the project.
    pub total_duration_ms: u32,
    pub created_at: String,
    pub updated_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_stored_trimmed() {
        assert_eq!(validate_name("  My podcast  ").unwrap(), "My podcast");
    }

    #[test]
    fn a_blank_name_is_refused() {
        assert_eq!(validate_name(""), Err(NameError::Empty));
        assert_eq!(validate_name("   "), Err(NameError::Empty));
        assert_eq!(validate_name("\t\n"), Err(NameError::Empty));
    }

    #[test]
    fn a_name_at_the_limit_is_accepted_and_one_over_is_not() {
        let at = "a".repeat(MAX_NAME_LEN);
        assert!(validate_name(&at).is_ok());
        let over = "a".repeat(MAX_NAME_LEN + 1);
        assert_eq!(validate_name(&over), Err(NameError::TooLong));
    }

    #[test]
    fn length_is_counted_in_characters_not_bytes() {
        // Each of these is 4 bytes and one character; a byte-counted limit
        // would reject a name a quarter of the allowed length.
        let emoji = "🎙".repeat(MAX_NAME_LEN);
        assert!(emoji.len() > MAX_NAME_LEN, "precondition: multi-byte");
        assert!(validate_name(&emoji).is_ok());
    }

    #[test]
    fn trimming_happens_before_the_length_check() {
        let padded = format!("  {}  ", "a".repeat(MAX_NAME_LEN));
        assert!(validate_name(&padded).is_ok(), "whitespace must not count");
    }

    #[test]
    fn every_name_error_explains_itself() {
        for e in [NameError::Empty, NameError::TooLong] {
            assert!(e.message().ends_with('.'), "{e:?}");
        }
    }

    #[test]
    fn a_project_round_trips() {
        let p = ProjectResponse {
            id: "p1".into(),
            name: "My podcast".into(),
            recording_count: 3,
            total_duration_ms: 125_000,
            created_at: "2026-10-09T00:00:00Z".into(),
            updated_at: "2026-10-09T00:00:00Z".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<ProjectResponse>(&json).unwrap(), p);
    }
}
