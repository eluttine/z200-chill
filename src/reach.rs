use std::fmt;

/// Shared reach-export metadata used by the checker and interpreter.
#[derive(Debug, Clone, Default)]
pub struct ExportMeta {
    /// Exported prefix; empty means the simple name is visible.
    pub prefix: Vec<String>,
    /// `TO` friend restriction; `None` means unrestricted.
    pub friends: Option<Vec<String>>,
}

impl ExportMeta {
    pub fn matches(&self, prefix: &[String], package: Option<&str>, module: Option<&str>) -> bool {
        self.prefix == prefix && self.is_accessible_to(package, module)
    }

    pub fn is_accessible_to(&self, package: Option<&str>, module: Option<&str>) -> bool {
        let Some(friends) = &self.friends else {
            return true;
        };
        let Some(module) = module else {
            return false;
        };
        let qualified = match package {
            Some(package) => format!("{package}!{module}"),
            None => module.to_owned(),
        };
        friends
            .iter()
            .any(|friend| friend == module || friend == &qualified)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectError {
    NotFound,
    Ambiguous,
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no such name in the enclosing reach"),
            Self::Ambiguous => f.write_str("ambiguous; several modules grant it"),
        }
    }
}

/// Select exactly one export matching the prefix and friend rules.
pub fn select<'a, T>(
    entries: impl IntoIterator<Item = (&'a ExportMeta, &'a T)>,
    prefix: &[String],
    package: Option<&str>,
    module: Option<&str>,
) -> Result<&'a T, SelectError> {
    let mut found = None;
    for (meta, value) in entries {
        if meta.matches(prefix, package, module) {
            if found.is_some() {
                return Err(SelectError::Ambiguous);
            }
            found = Some(value);
        }
    }
    found.ok_or(SelectError::NotFound)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_by_prefix_and_friends() {
        let simple = ExportMeta::default();
        let prefixed = ExportMeta {
            prefix: vec!["P".to_owned()],
            friends: None,
        };
        let restricted = ExportMeta {
            prefix: Vec::new(),
            friends: Some(vec!["Other".to_owned()]),
        };
        let entries = [(&simple, 1usize), (&prefixed, 2usize)];
        let entries = entries.iter().map(|(meta, value)| (*meta, value));

        assert_eq!(select(entries.clone(), &[], None, None).unwrap(), &1);
        assert_eq!(select(entries, &["P".to_owned()], None, None).unwrap(), &2);

        let restricted_entries = [(&restricted, 3usize)];
        let restricted_entries = restricted_entries
            .iter()
            .map(|(meta, value)| (*meta, value));
        assert_eq!(
            select(restricted_entries.clone(), &[], None, Some("Other")).unwrap(),
            &3
        );
        assert_eq!(
            select(restricted_entries, &[], None, Some("Stranger")),
            Err(SelectError::NotFound)
        );
    }

    #[test]
    fn detects_ambiguity() {
        let first = ExportMeta::default();
        let second = ExportMeta::default();
        let entries = [(&first, 1usize), (&second, 2usize)];
        let entries = entries.iter().map(|(meta, value)| (*meta, value));
        assert_eq!(
            select(entries, &[], None, None),
            Err(SelectError::Ambiguous)
        );
    }
}
