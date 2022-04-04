use dyn_clone::DynClone;

pub trait ObservableResource: DynClone + 'static {
    fn on_first_observer(&mut self, observers: Observers);
    fn on_last_observer(&mut self) -> Observers;
}

dyn_clone::clone_trait_object!(ObservableResource);

#[derive(Clone)]
pub struct Observers {}

impl Observers {
    pub fn notify_change(&self) {
        todo!()
    }
}
