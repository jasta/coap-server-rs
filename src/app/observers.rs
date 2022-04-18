use crate::app::u24::u24;
use std::mem;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::{watch, Mutex, RwLock};

/// Optional convenience mechanism to aid in managing dynamic [`Observers`] instances.
///
/// Intended to be used as:
/// ```no_run
/// use coap_server::app::{ObservableResource, Observers, ObserversHolder};
/// struct MyResource {
///     holder: ObserversHolder
/// }
/// #[async_trait::async_trait]
/// impl ObservableResource for MyResource {
///     async fn on_active(&self, observers: Observers) -> Observers {
///         self.holder.attach(observers).await;
///         self.holder.stay_active().await;
///         self.holder.detach().await
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ObserversHolder {
    inner: Arc<RwLock<Option<Observers>>>,
}

/// Handle that can be used to inform the server when changes are detected.
#[derive(Debug)]
pub struct Observers {
    notify_change_tx: Arc<watch::Sender<NotificationState>>,

    /// This will become the Observe value (i.e. the sequence number) if one is not provided
    /// by the handler directly.
    change_num: Arc<Mutex<u24>>,
}

#[derive(Debug, Copy, Clone)]
pub enum NotificationState {
    InitialSequence(u24),
    ResourceChanged(u24),
}

impl ObserversHolder {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(None)),
        }
    }

    /// Attach a new [`Observers`] instance which affects how [`notify_change`] behaves.
    pub async fn attach(&self, observers: Observers) {
        *self.inner.write().await = Some(observers);
    }

    /// Detach and return the owned [`Observers`] instance, meant to be sent back to
    /// [`crate::app::ObservableResource::on_active`].
    pub async fn detach(&self) -> Observers {
        mem::take(self.inner.write().await.deref_mut()).unwrap()
    }

    /// Keep an attached [`Observers`] instance active.  Panics if none is attached.
    pub async fn stay_active(&self) {
        self.inner
            .read()
            .await
            .as_ref()
            .unwrap()
            .stay_active()
            .await;
    }

    /// Defers to [`Observers::notify_change`] when attached; does nothing otherwise.
    pub async fn notify_change(&self) {
        if let Some(observers) = self.inner.read().await.as_ref() {
            observers.notify_change().await;
        }
    }
}

impl Default for ObserversHolder {
    fn default() -> Self {
        ObserversHolder::new()
    }
}

impl Observers {
    pub(crate) fn new(change_num: u24) -> Self {
        let init = NotificationState::InitialSequence(change_num);
        let (notify_change_tx, _) = watch::channel(init);
        Self {
            notify_change_tx: Arc::new(notify_change_tx),
            change_num: Arc::new(Mutex::new(change_num)),
        }
    }

    pub async fn stay_active(&self) {
        self.notify_change_tx.closed().await;
    }

    pub(crate) fn leak_notify_change_tx(&self) -> Arc<watch::Sender<NotificationState>> {
        self.notify_change_tx.clone()
    }

    /// Inform the server that a change to the underlying resource has potentially occurred.  The
    /// server responds by re-executing synthetic `Get` or `Fetch` requests roughly matching
    /// the original client request, then delivering the results to the peer.  Note that
    /// spurious changes will be delivered if this method is spammed so callers must take care
    /// to ensure it is only invoked when a genuine change is expected.
    ///
    /// Note that a sequence number will be generated for you if one is omitted
    /// from response in the re-executed request.  If you wish to provide your own,
    /// simply set the observe value in the response with `response.message.set_observe_value(...)`.
    /// Be sure that if you do this, you are taking care that the sequence number does not
    /// run backwards within 256 seconds as per:
    /// https://datatracker.ietf.org/doc/html/rfc7641#section-4.4
    pub async fn notify_change(&self) {
        let new_change_num = {
            let mut change_num = self.change_num.lock().await;
            *change_num = change_num.wrapping_add(u24::from(1u8));
            *change_num
        };
        let _ = self
            .notify_change_tx
            .send(NotificationState::ResourceChanged(new_change_num));
    }
}
