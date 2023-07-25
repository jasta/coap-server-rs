use alloc::boxed::Box;

use async_trait::async_trait;

use crate::app::observers::Observers;

/// Special manager-like type which is responsible for brokering support between the
/// core server infrastructure and individual resource handlers.
#[async_trait]
pub trait ObservableResource: 'static {
    /// Invoked when the first observer is registered for a given path (see
    /// [`Observers::relative_path`]).  An [`Observers`] instance
    /// is provided as a handle to inform the server when changes are detected which can be stored
    /// and used.  When this method completes all current observers for the given path will be
    /// removed.  For this reason, most customers should prefer to simply await on
    /// [`Observers::stay_active`] which ensures the method will not complete until all observers
    /// are gone.
    ///
    /// This method is a good place to initiate any potentially expensive watch operation
    /// that may be necessary to support observation of the associated resource.
    ///
    /// This method is guaranteed to not be called concurrently for any given path (i.e. `on_active`
    /// will block another call to `on_active`, but only for a specific path).
    async fn on_active(&self, observers: Observers) -> Observers;
}
