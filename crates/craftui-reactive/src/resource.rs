//! Async resource state helper for bridging background `crates.io` tasks to reactive signals.

/// Represents the lifecycle of an asynchronous operation (HTTP, DB, file I/O).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ResourceState<T> {
    /// No request has been started yet.
    #[default]
    Idle,
    /// A background operation is currently in flight.
    Loading,
    /// The operation completed successfully with a value.
    Ready(T),
    /// The operation failed with a human-readable error message.
    Error(String),
}

impl<T> ResourceState<T> {
    /// Returns `true` when the resource is currently loading.
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }

    /// Returns `true` when the resource holds a `Ready` value.
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    /// Returns `true` when the resource holds an `Error`.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    /// Returns a reference to the underlying value if `Ready`.
    pub fn data(&self) -> Option<&T> {
        match self {
            Self::Ready(value) => Some(value),
            _ => None,
        }
    }

    /// Returns the error message if `Error`.
    pub fn error(&self) -> Option<&str> {
        match self {
            Self::Error(msg) => Some(msg),
            _ => None,
        }
    }

    /// Maps a `ResourceState<T>` into `ResourceState<U>` while preserving `Idle`, `Loading`, and `Error`.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ResourceState<U> {
        match self {
            Self::Idle => ResourceState::Idle,
            Self::Loading => ResourceState::Loading,
            Self::Ready(v) => ResourceState::Ready(f(v)),
            Self::Error(e) => ResourceState::Error(e),
        }
    }
}

#[cfg(test)]
mod resource_tests {
    use super::ResourceState;

    #[test]
    fn resource_state_transitions_and_map() {
        let idle: ResourceState<i32> = ResourceState::default();
        assert_eq!(idle, ResourceState::Idle);
        assert!(!idle.is_loading());

        let loading: ResourceState<i32> = ResourceState::Loading;
        assert!(loading.is_loading());
        assert_eq!(loading.data(), None);

        let ready = ResourceState::Ready(21);
        assert!(ready.is_ready());
        assert_eq!(ready.data(), Some(&21));
        assert_eq!(ready.map(|v| v * 2), ResourceState::Ready(42));

        let err: ResourceState<i32> = ResourceState::Error("timeout".into());
        assert!(err.is_error());
        assert_eq!(err.error(), Some("timeout"));
    }
}
