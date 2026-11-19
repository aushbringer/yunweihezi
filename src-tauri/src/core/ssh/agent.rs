//! ssh-agent integration (OpenSSH agent: named pipe on Windows, SSH_AUTH_SOCK
//! on unix). `AgentClient` already implements russh's `Signer` trait, so it
//! can be passed straight to `authenticate_publickey_with`.

use russh::keys::agent::client::{AgentClient, AgentStream};

use crate::error::{AppError, AppResult};

pub type DynAgent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

#[cfg(windows)]
const OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// Connect to the platform's OpenSSH agent.
pub async fn connect_agent() -> AppResult<DynAgent> {
    #[cfg(windows)]
    {
        AgentClient::connect_named_pipe(OPENSSH_AGENT_PIPE)
            .await
            .map(|c| c.dynamic())
            .map_err(|e| AppError::Config(format!("ssh-agent named pipe: {e}")))
    }
    #[cfg(not(windows))]
    {
        AgentClient::connect_env()
            .await
            .map(|c| c.dynamic())
            .map_err(|e| AppError::Config(format!("ssh-agent (SSH_AUTH_SOCK): {e}")))
    }
}
