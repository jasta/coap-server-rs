use crate::app::{CoapError, Request, Response};
use async_trait::async_trait;
use dyn_clone::DynClone;
use std::future::Future;

#[async_trait]
pub trait RequestHandler<Endpoint>: DynClone + 'static {
    async fn handle(&self, request: Request<Endpoint>) -> Result<Response, CoapError>;
}

#[async_trait]
impl<Endpoint, F, R> RequestHandler<Endpoint> for F
where
    Endpoint: Send + Sync + 'static,
    F: Fn(Request<Endpoint>) -> R + Sync + Send + Clone + 'static,
    R: Future<Output = Result<Response, CoapError>> + Send,
{
    async fn handle(&self, request: Request<Endpoint>) -> Result<Response, CoapError> {
        (self)(request).await
    }
}
