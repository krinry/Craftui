//! Reactive form validation primitives (`ValidationRule`, `FieldValidator`, `FormGroup`).

use craftui_reactive::{Scope, Signal};

/// Declarative validation rules evaluated against string input values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationRule {
    Required(String),
    MinLength(usize, String),
    MaxLength(usize, String),
    Email(String),
    Contains(String, String),
}

impl ValidationRule {
    /// Evaluate this rule on `input`. Returns `Some(message)` on failure, or `None` when valid.
    pub fn check(&self, input: &str) -> Option<String> {
        match self {
            Self::Required(msg) => input.trim().is_empty().then(|| msg.clone()),
            Self::MinLength(min, msg) => (input.chars().count() < *min).then(|| msg.clone()),
            Self::MaxLength(max, msg) => (input.chars().count() > *max).then(|| msg.clone()),
            Self::Email(msg) => {
                let trimmed = input.trim();
                let valid = trimmed.split_once('@').is_some_and(|(local, domain)| {
                    !local.is_empty()
                        && !domain.is_empty()
                        && !domain.contains('@')
                        && domain.contains('.')
                        && !domain.starts_with('.')
                        && !domain.ends_with('.')
                });
                (!valid).then(|| msg.clone())
            }
            Self::Contains(needle, msg) => (!input.contains(needle.as_str())).then(|| msg.clone()),
        }
    }
}

/// Reactive field validator pairing a `Signal<String>` value with an error `Signal<Option<String>>`.
#[derive(Clone)]
pub struct FieldValidator {
    pub value: Signal<String>,
    pub error: Signal<Option<String>>,
    pub touched: Signal<bool>,
    rules: Vec<ValidationRule>,
}

impl FieldValidator {
    pub fn new(scope: Scope, value: Signal<String>, rules: Vec<ValidationRule>) -> Self {
        Self {
            value,
            error: scope.signal(None),
            touched: scope.signal(false),
            rules,
        }
    }

    /// Automatically re-validate whenever `value` changes after the field has been touched.
    pub fn bind_live(&self, scope: Scope) {
        let this = self.clone();
        scope.effect(move || {
            let v = this.value.get();
            if this.touched.get_untracked() {
                let err = this.first_error(&v);
                if this.error.get_untracked() != err {
                    this.error.set(err);
                }
            }
        });
    }

    fn first_error(&self, input: &str) -> Option<String> {
        self.rules.iter().find_map(|r| r.check(input))
    }

    /// Run all validation rules immediately, mark touched, update `error`, and return `true` if valid.
    pub fn validate(&self) -> bool {
        self.touched.set(true);
        let err = self.first_error(&self.value.get_untracked());
        let ok = err.is_none();
        self.error.set(err);
        ok
    }

    pub fn is_valid(&self) -> bool {
        self.first_error(&self.value.get_untracked()).is_none()
    }

    pub fn reset(&self) {
        self.touched.set(false);
        self.error.set(None);
    }
}

/// Group of [`FieldValidator`]s enabling atomic form-wide validation and reset.
#[derive(Clone, Default)]
pub struct FormGroup {
    fields: Vec<FieldValidator>,
}

impl FormGroup {
    pub fn new(fields: Vec<FieldValidator>) -> Self {
        Self { fields }
    }

    pub fn add(&mut self, field: FieldValidator) {
        self.fields.push(field);
    }

    /// Validate every field in the group (without short-circuiting so all field errors appear).
    pub fn validate_all(&self) -> bool {
        craftui_reactive::batch(|| {
            let mut all_ok = true;
            for f in &self.fields {
                if !f.validate() {
                    all_ok = false;
                }
            }
            all_ok
        })
    }

    pub fn is_valid(&self) -> bool {
        self.fields.iter().all(FieldValidator::is_valid)
    }

    pub fn reset_all(&self) {
        craftui_reactive::batch(|| {
            for f in &self.fields {
                f.reset();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use craftui_reactive::reset_runtime;

    #[test]
    fn field_and_form_group_validate_and_track_live() {
        reset_runtime();
        let scope = Scope::root();
        let email_sig = scope.signal("bad-email".to_string());
        let pass_sig = scope.signal("123".to_string());

        let email_v = FieldValidator::new(
            scope,
            email_sig,
            vec![
                ValidationRule::Required("Email required".into()),
                ValidationRule::Email("Invalid email".into()),
            ],
        );
        email_v.bind_live(scope);

        let pass_v = FieldValidator::new(
            scope,
            pass_sig,
            vec![ValidationRule::MinLength(8, "Min 8 chars".into())],
        );
        pass_v.bind_live(scope);

        let form = FormGroup::new(vec![email_v.clone(), pass_v.clone()]);
        assert!(!form.validate_all());
        assert_eq!(email_v.error.get_untracked(), Some("Invalid email".into()));
        assert_eq!(pass_v.error.get_untracked(), Some("Min 8 chars".into()));

        email_sig.set("user@craftui.dev".into());
        pass_sig.set("supersecret".into());
        assert_eq!(email_v.error.get_untracked(), None);
        assert_eq!(pass_v.error.get_untracked(), None);
        assert!(form.is_valid());
    }
}
