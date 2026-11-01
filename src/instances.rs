use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// How the container is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunType {
    /// The image's own entrypoint, with [`CreateInstance::args`] as its
    /// arguments. No ssh, no Jupyter — what a batch worker wants.
    #[default]
    Args,
    /// Vast's ssh proxy, with `onstart` run as a script.
    Ssh,
}

impl RunType {
    fn as_str(self) -> &'static str {
        match self {
            RunType::Args => "args",
            RunType::Ssh => "ssh_proxy",
        }
    }
}

/// What to rent an offer with: `PUT /asks/{offer_id}/`.
#[derive(Debug, Clone, Default)]
pub struct CreateInstance {
    pub image: String,
    /// Environment for the container. Values are passed as given; the API
    /// takes them as a JSON object, so nothing here needs shell quoting.
    pub env: Vec<(String, String)>,
    /// Disk, GB.
    pub disk_gb: f64,
    /// A label to find the instance by later. Vast stores it verbatim.
    pub label: Option<String>,
    /// `Some(price)` rents interruptibly at that bid **per GPU**, $/h;
    /// `None` rents on demand.
    pub bid_per_gpu: Option<f64>,
    pub runtype: RunType,
    /// Arguments to the image's entrypoint, for [`RunType::Args`].
    pub args: Vec<String>,
    /// A shell script run at start, for [`RunType::Ssh`]; with
    /// [`RunType::Args`] it replaces the entrypoint.
    pub onstart: Option<String>,
    /// `docker login` arguments for a private registry, as Vast takes them:
    /// `-u USER -p PASSWORD registry.example.com`.
    pub image_login: Option<String>,
    /// Fail rather than wait if the offer is gone by the time it is rented.
    pub cancel_unavailable: bool,
}

impl CreateInstance {
    pub(crate) fn to_body(&self) -> Value {
        let env: Map<String, Value> = self
            .env
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        let mut body = serde_json::json!({
            "client_id": "me",
            "image": self.image,
            "env": env,
            "disk": self.disk_gb,
            "runtype": self.runtype.as_str(),
            "cancel_unavail": self.cancel_unavailable,
        });
        let obj = body.as_object_mut().expect("a literal object");
        if let Some(price) = self.bid_per_gpu {
            obj.insert("price".into(), price.into());
        }
        if let Some(label) = &self.label {
            obj.insert("label".into(), label.clone().into());
        }
        if let Some(onstart) = &self.onstart {
            obj.insert("onstart".into(), onstart.clone().into());
        }
        if let Some(login) = &self.image_login {
            obj.insert("image_login".into(), login.clone().into());
        }
        if !self.args.is_empty() {
            obj.insert(
                "args".into(),
                Value::Array(self.args.iter().cloned().map(Value::String).collect()),
            );
        }
        body
    }
}

/// The answer to a rental: the new instance's id.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedInstance {
    #[serde(rename = "new_contract")]
    pub id: u64,
}

/// A rented instance.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Instance {
    pub id: u64,
    #[serde(default)]
    pub machine_id: Option<u64>,
    #[serde(default)]
    pub label: Option<String>,
    /// What the container is doing: `created`, `loading`, `running`,
    /// `exited`, `offline`, …; `null` before the host has reported.
    #[serde(default)]
    pub actual_status: Option<String>,
    /// What it was asked to do: `running` or `stopped`.
    #[serde(default)]
    pub intended_status: Option<String>,
    /// The contract's state: `running` or `stopped`. An interruptible
    /// instance that has been outbid is `stopped` here while
    /// `intended_status` is still `running`.
    #[serde(default)]
    pub cur_state: Option<String>,
    #[serde(default)]
    pub next_state: Option<String>,
    #[serde(default)]
    pub status_msg: Option<String>,
    #[serde(default)]
    pub is_bid: bool,
    #[serde(default)]
    pub gpu_name: Option<String>,
    #[serde(default)]
    pub num_gpus: Option<u32>,
    /// What it costs now, $/h, for the whole instance.
    #[serde(default)]
    pub dph_total: Option<f64>,
    #[serde(default)]
    pub min_bid: Option<f64>,
    #[serde(default)]
    pub image_uuid: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub start_date: Option<f64>,
    #[serde(default)]
    pub public_ipaddr: Option<String>,
    /// Vast's ssh proxy, for [`RunType::Ssh`] instances.
    #[serde(default)]
    pub ssh_host: Option<String>,
    #[serde(default)]
    pub ssh_port: Option<u16>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// An instance's condition, reduced to what a controller acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceStatus {
    /// Pulling the image or starting the container.
    Starting,
    Running,
    /// Interruptible and stopped: outbid, or the machine rented on demand.
    /// Still billed for storage; resumes only if the bid wins again. Vast
    /// reports this exactly as it reports an interruptible instance its owner
    /// stopped, so that reads as `Outbid` too.
    Outbid,
    /// Stopped because it was asked to be.
    Stopped,
    /// The container ran and ended.
    Exited,
    /// The host stopped reporting.
    Offline,
    /// Anything that says error.
    Failed,
}

impl Instance {
    pub fn status(&self) -> InstanceStatus {
        let actual = self.actual_status.as_deref().unwrap_or("");
        let intended = self.intended_status.as_deref().unwrap_or("running");
        let state = self.cur_state.as_deref().unwrap_or("");
        let msg = self
            .status_msg
            .as_deref()
            .unwrap_or("")
            .to_ascii_lowercase();

        if msg.contains("error") && actual != "running" {
            return InstanceStatus::Failed;
        }
        if actual == "offline" {
            return InstanceStatus::Offline;
        }
        // An interruptible whose contract is stopped: the market took it.
        // Vast says so in no field of its own, and it flips
        // `intended_status` to "stopped" as well — observed live, an RTX 5070
        // preempted when its machine was re-rented at twice the bid — so
        // this has to come before the intended-stop check. The cost is that
        // an interruptible instance you stopped yourself reads as outbid too.
        if self.is_bid && (state == "stopped" || msg.contains("outbid")) {
            return InstanceStatus::Outbid;
        }
        if intended == "stopped" {
            return InstanceStatus::Stopped;
        }
        match actual {
            "running" => InstanceStatus::Running,
            "exited" => InstanceStatus::Exited,
            "stopped" => InstanceStatus::Stopped,
            _ => InstanceStatus::Starting,
        }
    }

    /// GPUs on the instance, at least one.
    pub fn gpus(&self) -> u32 {
        self.num_gpus.unwrap_or(1).max(1)
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct InstancesResponse {
    pub instances: Vec<Instance>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct InstanceResponse {
    pub instances: Option<Instance>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn instance(v: Value) -> Instance {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_bid_rental_sends_the_price_and_the_env_as_an_object() {
        let body = CreateInstance {
            image: "registry.example/transcoder:latest".into(),
            env: vec![("A".into(), "1 2".into()), ("B".into(), "x=y".into())],
            disk_gb: 40.0,
            label: Some("fleet".into()),
            bid_per_gpu: Some(0.11),
            args: vec!["queue:work".into(), "--timeout=1800".into()],
            image_login: Some("-u u -p p registry.example".into()),
            ..Default::default()
        }
        .to_body();

        assert_eq!(body["client_id"], "me");
        assert_eq!(body["price"], 0.11);
        assert_eq!(body["env"], json!({"A": "1 2", "B": "x=y"}));
        assert_eq!(body["runtype"], "args");
        assert_eq!(body["args"], json!(["queue:work", "--timeout=1800"]));
        assert_eq!(body["label"], "fleet");
        assert_eq!(body["image_login"], "-u u -p p registry.example");
    }

    #[test]
    fn an_on_demand_rental_sends_no_price() {
        let body = CreateInstance {
            image: "x".into(),
            ..Default::default()
        }
        .to_body();
        assert!(body.get("price").is_none());
        assert!(body.get("args").is_none());
    }

    #[test]
    fn a_preempted_interruptible_reads_as_outbid_although_vast_says_stopped() {
        // The fields of a real preemption (instance 58593131, 2026-11-01): the
        // machine was re-rented at $0.173 over a $0.085 bid. Vast flipped
        // `intended_status` too, and left `min_bid` at the old floor.
        let preempted = instance(json!({
            "id": 58593131, "is_bid": true, "actual_status": "exited",
            "intended_status": "stopped", "cur_state": "stopped", "next_state": "stopped",
            "status_msg": "success, running ubuntu_24.04/ssh", "min_bid": 0.08,
        }));
        assert_eq!(preempted.status(), InstanceStatus::Outbid);
    }

    #[test]
    fn statuses_reduce_to_what_a_controller_acts_on() {
        let base = |extra: Value| {
            let mut v = json!({"id": 1, "is_bid": true, "intended_status": "running"});
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            instance(v)
        };
        assert_eq!(
            base(json!({"actual_status": "loading"})).status(),
            InstanceStatus::Starting
        );
        assert_eq!(base(json!({})).status(), InstanceStatus::Starting);
        assert_eq!(
            base(json!({"actual_status": "running", "cur_state": "running"})).status(),
            InstanceStatus::Running
        );
        assert_eq!(
            base(json!({"actual_status": "exited", "cur_state": "stopped"})).status(),
            InstanceStatus::Outbid
        );
        assert_eq!(
            base(json!({"actual_status": "offline"})).status(),
            InstanceStatus::Offline
        );
        assert_eq!(
            base(json!({"actual_status": "created", "status_msg": "Error response from daemon: pull access denied"})).status(),
            InstanceStatus::Failed
        );
        // An on-demand instance asked to stop is just stopped.
        let stopped = instance(
            json!({"id": 1, "is_bid": false, "intended_status": "stopped",
            "actual_status": "exited", "cur_state": "stopped"}),
        );
        assert_eq!(stopped.status(), InstanceStatus::Stopped);
        let on_demand = instance(
            json!({"id": 1, "is_bid": false, "intended_status": "running",
            "actual_status": "exited", "cur_state": "stopped"}),
        );
        assert_eq!(on_demand.status(), InstanceStatus::Exited);
    }
}
