use boxdesk_record::{End, Record, RunDir};
use pyo3::prelude::*;

// `skip_from_py_object`: these cross the boundary outward only. PyO3 0.29 deprecated the
// automatic `FromPyObject` on a `Clone` pyclass, and a record is something this SDK hands
// back, never something a caller passes in.
#[pyclass(module = "boxdesk.models", get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PyPosture {
    pub root: String,
    pub rootfs: String,
    pub mounts: Vec<(String, String)>,
    pub shares: Vec<(String, String)>,
    pub network: String,
    pub display: Option<String>,
    pub sound: bool,
    pub gpu: bool,
    pub results: bool,
    pub env: Vec<String>,
    pub vcpus: u8,
    pub mem_mib: u32,
}

#[pyclass(module = "boxdesk.models", get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PyFile {
    pub path: String,
    pub size_bytes: u64,
}

#[pyclass(module = "boxdesk.models", get_all, skip_from_py_object)]
#[derive(Clone)]
pub struct PyRun {
    pub run_id: String,
    pub name: String,
    pub verb: String,
    pub command: Vec<String>,
    pub posture: PyPosture,
    pub started_ms: Option<u64>,
    pub ended_ms: Option<u64>,
    pub end_kind: Option<String>,
    pub end_code: Option<i64>,
    pub pid: Option<u32>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub stdout_bytes: Option<u64>,
    pub stderr_bytes: Option<u64>,
    pub output_truncated: Option<bool>,
    pub files: Option<Vec<PyFile>>,
    pub dir: Option<String>,
    pub live: Option<bool>,
}

#[pymethods]
impl PyRun {
    #[getter]
    fn ok(&self) -> bool {
        self.end_kind.as_deref() == Some("exit") && self.end_code == Some(0)
    }
}

pub fn convert_record(record: &Record, dir: Option<&RunDir>, live: Option<bool>) -> PyRun {
    let (end_kind, end_code) = match record.end {
        Some(End::Exit(code)) => (Some("exit"), Some(i64::from(code))),
        Some(End::Signal(sig)) => (Some("signal"), Some(i64::from(sig))),
        Some(End::Stopped) => (Some("stopped"), None),
        Some(End::Gone) => (Some("gone"), None),
        Some(End::Failed) => (Some("failed"), None),
        Some(_) => (Some("unknown"), None),
        None => (None, None),
    };

    let p = &record.posture;
    let posture = PyPosture {
        root: p.root.display().to_string(),
        rootfs: p.rootfs.as_word().to_string(),
        mounts: p
            .mounts
            .iter()
            .map(|m| (m.guest.display().to_string(), m.host.display().to_string()))
            .collect(),
        shares: p
            .shares
            .iter()
            .map(|s| (s.tag.clone(), s.host.display().to_string()))
            .collect(),
        network: p.network.as_word().to_string(),
        display: p.display.map(|d| d.as_spec()),
        sound: p.sound,
        gpu: p.gpu,
        results: p.results,
        env: p.env.clone(),
        vcpus: p.vcpus.get(),
        mem_mib: p.mem_mib.get(),
    };

    let mut stdout = None;
    let mut stderr = None;
    let mut stdout_bytes = None;
    let mut stderr_bytes = None;
    let mut output_truncated = None;
    let mut files = None;
    let mut dir_str = None;

    if let Some(d) = dir {
        stdout = Some(std::fs::read_to_string(d.stdout()).unwrap_or_default());
        stderr = Some(std::fs::read_to_string(d.stderr()).unwrap_or_default());

        let size = |path: &std::path::Path| std::fs::metadata(path).map_or(0, |m| m.len());
        let cut = |path: &std::path::Path| path.with_extension("truncated").exists();

        let (out_path, err_path) = (d.stdout(), d.stderr());
        stdout_bytes = Some(size(&out_path));
        stderr_bytes = Some(size(&err_path));
        output_truncated = Some(cut(&out_path) || cut(&err_path));

        files = Some(
            d.result_files()
                .unwrap_or_default()
                .into_iter()
                .map(|(path, s)| PyFile {
                    path: path.display().to_string(),
                    size_bytes: s,
                })
                .collect(),
        );

        dir_str = Some(d.path().display().to_string());
    }

    PyRun {
        run_id: record.id.clone(),
        name: record.name.clone(),
        verb: record.verb.as_word().to_string(),
        command: record.command.clone(),
        posture,
        started_ms: Some(record.started_ms),
        ended_ms: record.ended_ms,
        end_kind: end_kind.map(|s| s.to_string()),
        end_code,
        pid: record.pid,
        stdout,
        stderr,
        stdout_bytes,
        stderr_bytes,
        output_truncated,
        files,
        dir: dir_str,
        live,
    }
}
