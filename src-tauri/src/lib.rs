mod commands;
mod core;
mod error;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "openterm=info,russh=warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            use tauri::Manager;
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("app data dir: {e}"))?;
            let state = state::AppState::new(&dir.join("openterm.db"))?;
            app.manage(state);
            app.manage(core::session::manager::SessionManager::new());
            app.manage(core::sftp::SftpManager::new());
            app.manage(core::ftp::FtpManager::new());
            app.manage(core::ssh::forward::TunnelManager::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::session_cmds::session_open,
            commands::session_cmds::ssh_test_connect,
            commands::session_cmds::session_attach,
            commands::session_cmds::session_input,
            commands::session_cmds::session_resize,
            commands::session_cmds::session_close,
            commands::session_cmds::local_shell_open,
            commands::session_cmds::telnet_open,
            commands::session_cmds::rlogin_open,
            commands::session_cmds::serial_open,
            commands::session_cmds::serial_list_ports,
            commands::session_cmds::serial_zsend,
            commands::session_cmds::serial_xsend,
            commands::session_cmds::serial_xrecv,
            commands::tunnel_cmds::tunnel_start,
            commands::tunnel_cmds::tunnel_stop,
            commands::tunnel_cmds::tunnel_running,
            commands::tunnel_cmds::tunnel_saved_list,
            commands::tunnel_cmds::tunnel_saved_upsert,
            commands::tunnel_cmds::tunnel_saved_delete,
            commands::log_cmds::log_list,
            commands::log_cmds::log_delete,
            commands::log_cmds::log_tail,
            commands::log_cmds::log_open_folder,
            commands::crypto_cmds::crypto_status,
            commands::crypto_cmds::crypto_set_master,
            commands::crypto_cmds::crypto_unlock,
            commands::crypto_cmds::crypto_lock,
            commands::crypto_cmds::credential_list,
            commands::crypto_cmds::credential_create,
            commands::crypto_cmds::credential_update,
            commands::crypto_cmds::credential_secret,
            commands::crypto_cmds::credential_delete,
            commands::config_cmds::group_list,
            commands::config_cmds::group_upsert,
            commands::config_cmds::group_delete,
            commands::config_cmds::session_list,
            commands::config_cmds::session_upsert,
            commands::config_cmds::session_delete,
            commands::config_cmds::setting_get,
            commands::config_cmds::setting_set,
            commands::config_cmds::quick_command_list,
            commands::config_cmds::quick_command_upsert,
            commands::config_cmds::quick_command_delete,
            commands::config_cmds::trigger_list,
            commands::config_cmds::trigger_upsert,
            commands::config_cmds::trigger_delete,
            commands::config_cmds::config_export,
            commands::config_cmds::config_import,
            commands::sftp_cmds::sftp_open,
            commands::sftp_cmds::sftp_close,
            commands::sftp_cmds::external_edit_open,
            commands::sftp_cmds::sftp_home,
            commands::sftp_cmds::sftp_list,
            commands::sftp_cmds::sftp_mkdir,
            commands::sftp_cmds::sftp_delete,
            commands::sftp_cmds::sftp_rename,
            commands::sftp_cmds::sftp_watch,
            commands::sftp_cmds::sftp_download,
            commands::sftp_cmds::sftp_upload,
            commands::sftp_cmds::sftp_cancel,
            commands::sftp_cmds::scp_upload,
            commands::sftp_cmds::scp_download,
            commands::ftp_cmds::ftp_open,
            commands::ftp_cmds::ftp_close,
            commands::ftp_cmds::ftp_list,
            commands::ftp_cmds::ftp_mkdir,
            commands::ftp_cmds::ftp_delete,
            commands::ftp_cmds::ftp_rename,
            commands::ftp_cmds::ftp_download,
            commands::ftp_cmds::ftp_upload,
            commands::fs_cmds::fs_list,
            commands::fs_cmds::fs_home,
            commands::fs_cmds::fs_mkdir,
            commands::fs_cmds::fs_delete,
            commands::fs_cmds::fs_rename,
            commands::fs_cmds::fs_roots,
            commands::fs_cmds::text_file_read,
            commands::fs_cmds::text_file_write,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
