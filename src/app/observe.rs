use dyn_clone::DynClone;

/// Special manager-like type which is responsible for brokering support between the
/// core server infrastructure and individual resource handlers.  Uses an ownership
/// transfer scheme to ensure that callers cannot leak [`Observers`] instances.
pub trait ObservableResource: DynClone + 'static {
    /// Invoked when the first observer is registered.  An [`Observers`] instance is provided
    /// as a handle to inform the server when changes are detected.
    ///
    /// This method is a good place to initiate any potentially expensive watch operation
    /// that may be necessary to support observation of the associated resource.
    ///
    /// Multiple calls to this method are expected but always when balanced with interleaved
    /// calls to [`ObservableResource::on_last_observer`].
    fn on_first_observer(&mut self, observers: Observers);

    /// Invoked when the last observer is unregistered.  The [`Observers`] instance originally
    /// given to you in [`ObservableResource::on_first_observer`] must be returned back to the
    /// callers, thus denying further access to notify changes.
    fn on_last_observer(&mut self) -> Observers;
}

dyn_clone::clone_trait_object!(ObservableResource);

/// Handle that can be used to inform the server when changes are detected.
#[derive(Clone)]
pub struct Observers {}

impl Observers {
    /// Inform the server that a change to the underlying resource has potentially occurred.  The
    /// server responds by re-executing synthetic `Get` or `Fetch` requests roughly matching
    /// the original client request, then delivering the results to the peer.  Note that
    /// spurious changes will be delivered if this method is spammed so callers must take care
    /// to ensure it is only invoked when a genuine change is expected.
    pub fn notify_change(&self) {
        todo!()
    }
}
