use std::time::Duration;
use anyhow::{anyhow, Result};
use reqwest::Client;
use tokio::runtime::Handle;
use tokio::time::timeout;
use v8vm::vm::Resolver;
use v8vm::ex::fetch::{self, Request, Response};

pub struct HttpClient {
    client: Client,
    handle: Handle,
}

impl HttpClient {
    pub fn new(handle: Handle) -> Self {
        let client = Client::new();
        Self { client, handle }
    }

    async fn send(client: Client, request: Request) -> Result<Response> {
        let method = request.method.parse()?;
        let url    = request.url.parse()?;

        let request  = reqwest::Request::new(method, url);
        let response = client.execute(request).await?;
        let status   = response.status();
        let body     = response.text().await?;

        Ok(Response {
            status: status,
            body:   body,
        })
    }
}

impl fetch::Client for HttpClient {
    fn fetch(&self, request: Request, resolver: Resolver) {
        let client = self.client.clone();
        self.handle.spawn(async move {
            let expiry = Duration::from_secs(10);

            let result = Self::send(client, request);

            match timeout(expiry, result).await {
                Ok(Ok(r))  => resolver.resolve(Box::new(r)),
                Ok(Err(e)) => resolver.reject(Box::new(e)),
                Err(_)     => resolver.reject(Box::new(anyhow!("timeout"))),
            }
        });
    }
}
