//! Script execution for trigger `script` actions and keyboard macros.
//!
//! Python support lives behind the `python` cargo feature (PyO3, abi3).
//! Without the feature, script actions report a clear message back to the
//! session instead of failing silently.

use crate::core::terminal::pipe::SessionPipe;
use crate::core::terminal::trigger::TriggerFire;

/// Execute a trigger `script` action. `param` is either inline Python source
/// or `@path/to/script.py` to load from disk.
pub fn run_trigger_script(pipe: &SessionPipe, fire: &TriggerFire) {
    #[cfg(feature = "python")]
    {
        match python::run(fire) {
            Ok(out) => {
                if !out.is_empty() {
                    pipe.push_data(format!("\r\n\x1b[36m[script {}]\x1b[0m\r\n{out}", fire.name).as_bytes());
                }
            }
            Err(e) => {
                pipe.push_data(
                    format!("\r\n\x1b[31m[script {} failed: {e}]\x1b[0m\r\n", fire.name).as_bytes(),
                );
            }
        }
    }
    #[cfg(not(feature = "python"))]
    {
        pipe.push_data(
            format!(
                "\r\n\x1b[33m[trigger \"{}\" script action skipped: build without `python` feature]\x1b[0m\r\n",
                fire.name
            )
            .as_bytes(),
        );
    }
}

#[cfg(feature = "python")]
pub mod python {
    use crate::core::terminal::trigger::TriggerFire;
    use pyo3::prelude::*;
    use pyo3::types::PyDict;

    /// Run the script with `match` / `trigger` / `param` variables in scope.
    /// Returns captured stdout (via a redirected `sys.stdout` StringIO).
    pub fn run(fire: &TriggerFire) -> anyhow::Result<String> {
        let src = if let Some(path) = fire.param.strip_prefix('@') {
            std::fs::read_to_string(path)?
        } else {
            fire.param.clone()
        };
        let code = std::ffi::CString::new(src)?;
        Python::attach(|py| {
            let globals = PyDict::new(py);
            globals.set_item("match", &fire.matched)?;
            globals.set_item("trigger_name", &fire.name)?;
            let io = py.import("io")?;
            let buf = io.call_method0("StringIO")?;
            let sys = py.import("sys")?;
            let old_stdout = sys.getattr("stdout")?;
            sys.setattr("stdout", &buf)?;
            let result = py.run(code.as_c_str(), Some(&globals), None);
            sys.setattr("stdout", old_stdout)?;
            result?;
            let out: String = buf.call_method0("getvalue")?.extract()?;
            Ok(out)
        })
    }
}
