use crate::app::path_matcher::{key_from_path, PathMatcher};
use crate::app::u24::u24;
use std::sync::Arc;
use log::trace;
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
///         let attached = self.holder.attach(observers).await;
///         attached.stay_active().await;
///         attached.detach().await
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ObserversHolder {
    inner: Arc<RwLock<PathMatcher<Arc<Observers>>>>,
}

/// Handle that can be used to inform the server when changes are detected.
#[derive(Debug)]
pub struct Observers {
    relative_path_key: Vec<String>,

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
            inner: Arc::new(RwLock::new(PathMatcher::new_empty())),
        }
    }

    /// Attach a new [`Observers`] instance which affects how [`notify_change`] behaves.
    pub async fn attach(&self, observers: Observers) -> Attached<'_> {
        let key = observers.relative_path_key.clone();
        let observers_arc = Arc::new(observers);
        self.inner.write().await.insert(key.clone(), observers_arc.clone());
        Attached { key, value: observers_arc, holder: self }
    }

    /// Defers to [`Observers::notify_change`] when attached; does nothing otherwise.
    pub async fn notify_change(&self) {
        for observers in self.inner.read().await.values() {
            observers.notify_change().await;
        }
    }

    /// Special variation of [`notify_change`] that indicates only observe requests grouped
    /// under the provided path should be notified of the change.  This optimization can help a lot
    /// when you are observing dynamic resources (i.e. /resources/{resource_name}/) with a very
    /// large number of updates across different resources.
    ///
    /// The provided `relative_path` is used for fuzzy matching of any "relevant" observing path.  For
    /// example, if `relative_path` is `"resources/abc"` then it will match against observe requests for
    /// `"resources/abc/some_property"`, `"resources/abc"`, or even `"resources"`.  It would not
    /// match observe requests for `"/resources/xyz"`.
    ///
    /// `relative_path` is relative to the resource path that the [`crate::app::ObservableResource`]
    /// was installed at.
    pub async fn notify_change_for_path(&self, relative_path: &str) {
        trace!("entered notify_change_for_path: {relative_path}");
        for result in self
            .inner
            .read()
            .await
            .match_all(&key_from_path(relative_path))
        {
            trace!("entered notify_change");
            result.value.notify_change().await;
            trace!("...exit");
        }
        trace!("...exit");
    }
}

pub struct Attached<'a> {
    key: Vec<String>,
    value: Arc<Observers>,
    holder: &'a ObserversHolder,
}

impl<'a> Attached<'a> {
    /// Keep an attached [`Observers`] instance active.  Panics if none is attached.
    pub async fn stay_active(&self) {
        self.value.stay_active().await;
    }

    /// Detach and return the owned [`Observers`] instance, meant to be sent back to
    /// [`crate::app::ObservableResource::on_active`].
    pub async fn detach(self) -> Observers {
        self.holder.inner.write().await.remove(&self.key).unwrap();
        Arc::try_unwrap(self.value).unwrap()
    }
}

impl Default for ObserversHolder {
    fn default() -> Self {
        ObserversHolder::new()
    }
}

impl Observers {
    pub(crate) fn new(relative_path_key: Vec<String>, change_num: u24) -> Self {
        let init = NotificationState::InitialSequence(change_num);
        let (notify_change_tx, _) = watch::channel(init);
        Self {
            relative_path_key,
            notify_change_tx: Arc::new(notify_change_tx),
            change_num: Arc::new(Mutex::new(change_num)),
        }
    }

    pub fn relative_path(&self) -> String {
        self.relative_path_key.join("/")
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
    /// [RFC 7641, section 4.4](https://datatracker.ietf.org/doc/html/rfc7641#section-4.4)
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
