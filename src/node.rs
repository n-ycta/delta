//! Assembles a node: config -> kernel -> built-in components -> transport -> ready.
use crate::components::{Core, Peers, Tokens, Identity};
#[cfg(feature = "native-processes")]
use crate::components::{Exec, Supervisor};
#[cfg(feature = "host-telemetry")]
use crate::components::System;
use crate::config::Config;
use crate::errors::{DeltaError, Result};
use crate::kernel::{Component, Kernel};
use crate::server;
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};

/// Explicit assembly policy; compiled desktop support never implies mobile permission.
#[derive(Clone,Copy,Debug)]
pub struct HostPolicy { pub processes: bool, pub telemetry: bool }
impl HostPolicy { pub const DESKTOP: Self = Self {processes:true,telemetry:true}; pub const MOBILE: Self = Self {processes:false,telemetry:false}; }
pub struct Node {
    pub config: Arc<Config>,
    pub kernel: Arc<Kernel>,
    port: u16,
    shutdown: Option<oneshot::Sender<()>>,
    server: Option<JoinHandle<()>>,
}

impl Node {
    /// Start a node. `port` overrides the configured port (use `Some(0)` for a random one).
    pub async fn start(config: Arc<Config>, port: Option<u16>) -> Result<Node> {
        Self::start_with_policy(config,port,HostPolicy::DESKTOP).await
    }
    pub async fn start_with_policy(config: Arc<Config>, port: Option<u16>, policy:HostPolicy) -> Result<Node> {
        let _ = policy;
        let (listen, cfg_port, tls) = config.read(|c| (c.listen.clone(), c.port, c.tls_cert.is_some()));
        if tls {
            eprintln!("delta: tls_cert is set, but this build serves plain HTTP; put a TLS proxy or WireGuard in front");
        }
        let listener = TcpListener::bind((listen.as_str(), port.unwrap_or(cfg_port)))
            .await
            .map_err(|e| DeltaError::unavailable(format!("cannot bind {listen}: {e}")))?;
        let port = listener.local_addr().map_err(|e| DeltaError::unavailable(e.to_string()))?.port();

        let kernel = Kernel::new(config.clone());
        kernel.start();
        kernel.set_local_url(format!("http://127.0.0.1:{port}"));

        let b = config.read(|c| c.builtin.clone());
        if b.core {
            kernel.add(Arc::new(Core)).await?;
        }
        if b.tokens {
            kernel.add(Arc::new(Tokens)).await?;
        }
        kernel.add(Arc::new(Identity::new())).await?;
        kernel.add(Arc::new(crate::components::Dependencies)).await?;
        #[cfg(feature = "native-processes")]
        if policy.processes {kernel.add(Arc::new(crate::components::Packages::new())).await?;}
        if b.peers {
            let peers: Arc<dyn Component> = Peers::new(config.clone());
            kernel.add(peers).await?;
        }
        #[cfg(feature = "host-telemetry")]
        if b.system && policy.telemetry {
            kernel.add(Arc::new(System)).await?;
        }
        #[cfg(feature = "native-processes")]
        if b.exec && policy.processes {
            kernel.add(Arc::new(Exec::new(config.clone()))).await?;
        }
        #[cfg(feature = "native-processes")]
        if b.supervisor && policy.processes {
            // last: starts external processes
            let sup: Arc<dyn Component> = Supervisor::new(config.clone());
            kernel.add(sup).await?;
        }

        let (tx, rx) = oneshot::channel::<()>();
        let app = server::app(kernel.clone());
        let handle = tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = rx.await;
                })
                .await;
        });
        kernel.set_ready();
        Ok(Node { config, kernel, port, shutdown: Some(tx), server: Some(handle) })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub async fn stop(mut self) {
        self.kernel.stop().await;
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(mut h) = self.server.take() {
            if tokio::time::timeout(Duration::from_secs(3), &mut h).await.is_err() {
                h.abort();
            }
        }
    }

    pub async fn serve_forever(self) {
        wait_for_signal().await;
        self.stop().await;
    }
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(_) => {
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() { let _ = tokio::signal::ctrl_c().await; }
