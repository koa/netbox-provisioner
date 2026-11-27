//! Scope: The NetBox helpers tried against the lab (E9); not the helpers themselves
//! (`backend::helper`), nor the CHR harness (`chr_network`).
//!
//! Each case plans a helper over the running provisioner's GraphQL as an admin and writes it to
//! the lab's NetBox; the automatic rollout brings the lab's devices along. It checks that each
//! device got the script the plan's preview showed for it (`NetboxPlan.previewAccess`, against
//! the rollout's `ApplyPlanned`), that every device is like NetBox afterwards, and the same
//! once the write is undone, with the plan as before (or a helper taking it out again, like
//! „Abräumen“ after „Verbindung“, „Kunden-Port abräumen“ after „Kunden-Netz“). It stops before writing at a device not
//! like NetBox and at a plan announcing something outward (`announcesOutward`), which the
//! rollout would hold back anyway.
//!
//! Needs the lab running: NetBox (`local/netbox/`), Keycloak (`local/keycloak/`) and the
//! provisioner (`local/lab/run-provisioner.sh`), whose site `LAB_SITE` rolls out
//! automatically. Ignored, never in CI: `cargo test -p backend --test lab_helpers -- --ignored
//! --test-threads 1`.

use anyhow::{Context, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

/// How long a rollout may take: the webhook's quiet time, a round reading every device, the
/// applies with their check.
const ROLLOUT_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const POLL: Duration = Duration::from_secs(5);

/// The running lab provisioner, asked as an admin.
struct Lab {
    http: reqwest::Client,
    graphql: String,
    token_url: String,
    user: String,
    password: String,
    site: u32,
}

/// A RouterOS device of the lab's site.
#[derive(Debug, Clone)]
struct LabDevice {
    id: u32,
    name: String,
}

/// What a helper's plan would bring each device (masked, [`masked`]), and whether it would
/// announce something outward.
struct Preview {
    fingerprint: String,
    scripts: BTreeMap<String, String>,
    outward: Vec<String>,
}

impl Preview {
    /// The devices the plan changes.
    fn changed(&self) -> BTreeSet<String> {
        self.scripts
            .iter()
            .filter(|(_, script)| !script.is_empty())
            .map(|(name, _)| name.clone())
            .collect()
    }
}

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}

impl Lab {
    fn from_env() -> Lab {
        Lab {
            http: reqwest::Client::new(),
            graphql: env("LAB_PROVISIONER", "http://127.0.0.1:8082") + "/graphql",
            token_url: env("LAB_KEYCLOAK", "http://localhost:8180/realms/lab")
                + "/protocol/openid-connect/token",
            user: env("LAB_USER", "admin"),
            password: env("LAB_PASSWORD", "admin"),
            site: env("LAB_SITE", "1").parse().expect("LAB_SITE: a site id"),
        }
    }

    async fn query(&self, query: &str) -> anyhow::Result<Value> {
        let token: Value = self
            .http
            .post(&self.token_url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form(&[
                ("grant_type", "password"),
                ("client_id", "netbox-provisioner"),
                ("username", &self.user),
                ("password", &self.password),
                ("scope", "openid"),
            ]))
            .send()
            .await
            .context("Keycloak")?
            .json()
            .await?;
        let token = token["access_token"].as_str().context("no access token")?;
        let answer: Value = self
            .http
            .post(&self.graphql)
            .bearer_auth(token)
            .json(&json!({ "query": query }))
            .send()
            .await
            .context("the provisioner")?
            .json()
            .await?;
        if let Some(errors) = answer["errors"].as_array().filter(|e| !e.is_empty()) {
            bail!("{query}: {errors:?}");
        }
        Ok(answer["data"].clone())
    }

    async fn routeros_devices(&self) -> anyhow::Result<Vec<LabDevice>> {
        let data = self
            .query(&format!(
                "{{ topology {{ siteById(id: {}) {{ devices {{ id name hasRouteros }} }} }} }}",
                self.site
            ))
            .await?;
        Ok(data["topology"]["siteById"]["devices"]
            .as_array()
            .context("the site")?
            .iter()
            .filter(|device| device["hasRouteros"] == true)
            .map(|device| LabDevice {
                id: device["id"].as_u64().unwrap_or_default() as u32,
                name: device["name"].as_str().unwrap_or_default().into(),
            })
            .collect())
    }

    async fn device_id(&self, name: &str) -> anyhow::Result<u32> {
        let data = self
            .query("{ topology { allDevices { id name } } }")
            .await?;
        data["topology"]["allDevices"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|device| device["name"] == name)
            .and_then(|device| device["id"].as_u64())
            .map(|id| id as u32)
            .with_context(|| format!("no device {name}"))
    }

    async fn interface_id(&self, device: u32, name: &str) -> anyhow::Result<u32> {
        let data = self
            .query(&format!(
                "{{ topology {{ deviceById(id: {device}) {{ interfaceView {{ rows {{ interface {{ id name }} }} }} }} }} }}"
            ))
            .await?;
        data["topology"]["deviceById"]["interfaceView"]["rows"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| &row["interface"])
            .find(|interface| interface["name"] == name)
            .and_then(|interface| interface["id"].as_u64())
            .map(|id| id as u32)
            .with_context(|| format!("no interface {name} at {device}"))
    }

    /// The id of the lab's site prefix `prefix` „Kunden-Netz“ offers.
    async fn prefix_id(&self, prefix: &str) -> anyhow::Result<u32> {
        let device = self.device_id("lab-crs226").await?;
        let data = self
            .query(&format!(
                "{{ topology {{ deviceById(id: {device}) {{ customerNetOptions {{ parents {{ id prefix }} parentsV6 {{ id prefix }} }} }} }} }}"
            ))
            .await?;
        let options = &data["topology"]["deviceById"]["customerNetOptions"];
        ["parents", "parentsV6"]
            .iter()
            .flat_map(|list| options[list].as_array().cloned().unwrap_or_default())
            .find(|parent| parent["prefix"] == prefix)
            .and_then(|parent| parent["id"].as_u64())
            .map(|id| id as u32)
            .with_context(|| format!("no prefix {prefix}"))
    }

    /// The script an apply would run on `device` now, read anew.
    async fn diff(&self, device: &LabDevice) -> anyhow::Result<String> {
        let data = self
            .query(&format!(
                "{{ topology {{ deviceById(id: {}) {{ access(maxAge: 0) {{ generateCfg {{ diff {{ script }} }} }} }} }} }}",
                device.id
            ))
            .await?;
        let script = &data["topology"]["deviceById"]["access"]["generateCfg"]["diff"]["script"];
        Ok(masked(
            script
                .as_str()
                .with_context(|| format!("{} not reached", device.name))?,
        ))
    }

    /// Fails with the devices whose diff isn't empty, one after the other (a device's reads
    /// stay sequential anyway).
    async fn all_like_netbox(&self, devices: &[LabDevice]) -> anyhow::Result<()> {
        let mut unlike = Vec::new();
        for device in devices {
            let script = self.diff(device).await?;
            if !script.is_empty() {
                unlike.push(format!("{}:\n{script}", device.name));
            }
        }
        ensure!(unlike.is_empty(), "not like NetBox:\n{}", unlike.join("\n"));
        Ok(())
    }

    async fn plan_fingerprint(&self, helper: &str) -> anyhow::Result<(String, usize)> {
        let data = self
            .query(&format!(
                "{{ netboxPlan(helper: {helper}) {{ fingerprint steps {{ __typename }} }} }}"
            ))
            .await?;
        let plan = &data["netboxPlan"];
        Ok((
            plan["fingerprint"].as_str().unwrap_or_default().into(),
            plan["steps"].as_array().map_or(0, Vec::len),
        ))
    }

    /// The script `device` would get after `helper`'s plan, applied together with `together`.
    async fn preview_of(
        &self,
        helper: &str,
        device: &LabDevice,
        together: &[u32],
    ) -> anyhow::Result<(String, bool)> {
        let data = self
            .query(&format!(
                "{{ netboxPlan(helper: {helper}) {{ previewAccess(device: {}) {{ generateCfg(together: {together:?}) {{ diff {{ script }} announcesOutward }} }} }} }}",
                device.id
            ))
            .await?;
        let generated = &data["netboxPlan"]["previewAccess"]["generateCfg"];
        let script = generated["diff"]["script"]
            .as_str()
            .with_context(|| format!("{} not reached", device.name))?;
        Ok((masked(script), generated["announcesOutward"] == true))
    }

    /// What `helper`'s plan brings every device: alone first, then the changed ones together,
    /// as the rollout applies them.
    async fn preview(&self, helper: &str, devices: &[LabDevice]) -> anyhow::Result<Preview> {
        let (fingerprint, steps) = self.plan_fingerprint(helper).await?;
        ensure!(steps > 0, "{helper}: nothing to change");
        let mut scripts = BTreeMap::new();
        for device in devices {
            let (script, _) = self.preview_of(helper, device, &[]).await?;
            scripts.insert(device.name.clone(), script);
        }
        let together = devices
            .iter()
            .filter(|device| !scripts[&device.name].is_empty())
            .collect::<Vec<_>>();
        let ids = together.iter().map(|device| device.id).collect::<Vec<_>>();
        let mut outward = Vec::new();
        for device in together {
            let (script, announces) = self.preview_of(helper, device, &ids).await?;
            scripts.insert(device.name.clone(), script);
            if announces {
                outward.push(device.name.clone());
            }
        }
        Ok(Preview {
            fingerprint,
            scripts,
            outward,
        })
    }

    async fn job_ids(&self, devices: &[LabDevice]) -> anyhow::Result<BTreeSet<u32>> {
        let mut ids = BTreeSet::new();
        for device in devices {
            let data = self
                .query(&format!("{{ jobs(deviceId: {}) {{ id }} }}", device.id))
                .await?;
            ids.extend(
                data["jobs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|job| job["id"].as_u64())
                    .map(|id| id as u32),
            );
        }
        Ok(ids)
    }

    /// Waits for the job `id`; fails unless it is done.
    async fn job_done(&self, id: u32) -> anyhow::Result<()> {
        loop {
            let data = self
                .query(&format!(
                    "{{ job(id: {id}) {{ state error {{ code message }} }} }}"
                ))
                .await?;
            match data["job"]["state"].as_str() {
                Some("RUNNING") => tokio::time::sleep(Duration::from_secs(1)).await,
                Some("DONE") => return Ok(()),
                other => bail!("job {id}: {other:?} {}", data["job"]["error"]),
            }
        }
    }

    async fn write(&self, helper: &str, fingerprint: &str) -> anyhow::Result<u32> {
        let data = self
            .query(&format!(
                "mutation {{ writeNetboxPlan(helper: {helper}, fingerprint: {fingerprint:?}) {{ id }} }}"
            ))
            .await?;
        let job = data["writeNetboxPlan"]["id"].as_u64().context("a job")? as u32;
        self.job_done(job).await?;
        Ok(job)
    }

    async fn undo(&self, job: u32) -> anyhow::Result<()> {
        let data = self
            .query(&format!(
                "mutation {{ undoNetboxWrite(jobId: {job}) {{ id }} }}"
            ))
            .await?;
        let undo = data["undoNetboxWrite"]["id"].as_u64().context("a job")? as u32;
        self.job_done(undo).await
    }

    /// Waits for the rollout's first round after `since` that does something: it must apply
    /// to `changed` (any, or none, where it is `None`), or have nothing to do where nothing
    /// changes. A round that found a device busy or the internet away is waited past; one
    /// holding back, failing or not reading NetBox fails.
    async fn rolled_out(
        &self,
        since: Instant,
        changed: Option<&BTreeSet<String>>,
    ) -> anyhow::Result<()> {
        let query = format!(
            "{{ topology {{ siteById(id: {}) {{ autoRollout {{ error {{ __typename }} lastRound {{ age outcome applied {{ name }} heldBy {{ name }} held {{ device {{ name }} reasons }} deferred {{ name }} }} }} }} }} }}",
            self.site
        );
        let deadline = Instant::now() + ROLLOUT_TIMEOUT;
        loop {
            ensure!(Instant::now() < deadline, "no rollout in time");
            let data = self.query(&query).await?;
            let rollout = &data["topology"]["siteById"]["autoRollout"];
            ensure!(rollout["error"].is_null(), "rollout: {}", rollout["error"]);
            let round = &rollout["lastRound"];
            let after = round["age"]
                .as_u64()
                .is_some_and(|age| Duration::from_secs(age) < since.elapsed());
            if after {
                let applied = round["applied"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|device| device["name"].as_str())
                    .map(String::from)
                    .collect::<BTreeSet<_>>();
                match round["outcome"].as_str() {
                    Some("APPLIED") => {
                        let Some(changed) = changed else {
                            return Ok(());
                        };
                        ensure!(
                            &applied == changed,
                            "applied to {applied:?}, the preview changes {changed:?}"
                        );
                        return Ok(());
                    }
                    Some("NOTHING_TO_DO") => {
                        ensure!(
                            changed.is_none_or(BTreeSet::is_empty),
                            "nothing applied, the preview changes {changed:?}"
                        );
                        return Ok(());
                    }
                    Some("BUSY" | "NO_INTERNET") => {}
                    _ => bail!("rollout: {round}"),
                }
            }
            tokio::time::sleep(POLL).await;
        }
    }

    /// The scripts the jobs of `devices` not in `before` applied, by device (masked).
    async fn applied(
        &self,
        devices: &[LabDevice],
        before: &BTreeSet<u32>,
    ) -> anyhow::Result<BTreeMap<String, String>> {
        let mut applied = BTreeMap::new();
        for device in devices {
            let data = self
                .query(&format!(
                    "{{ jobs(deviceId: {}) {{ id events {{ event {{ __typename ... on ApplyPlanned {{ script }} }} }} }} }}",
                    device.id
                ))
                .await?;
            let scripts = data["jobs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|job| {
                    job["id"]
                        .as_u64()
                        .is_some_and(|id| !before.contains(&(id as u32)))
                })
                .flat_map(|job| job["events"].as_array().cloned().unwrap_or_default())
                .filter_map(|logged| logged["event"]["script"].as_str().map(masked))
                .collect::<Vec<_>>();
            match &scripts[..] {
                [] => {}
                [script] => {
                    applied.insert(device.name.clone(), script.clone());
                }
                _ => bail!("{}: more than one apply: {scripts:#?}", device.name),
            }
        }
        Ok(applied)
    }

    /// Writes `helper`'s plan and checks the rollout against the preview: the write's job
    /// and the devices it changes, also where the check failed (the error), to take it back.
    async fn write_checked(&self, helper: &str) -> anyhow::Result<Written> {
        let devices = self.routeros_devices().await?;
        self.all_like_netbox(&devices).await?;
        let preview = self.preview(helper, &devices).await?;
        ensure!(
            preview.outward.is_empty(),
            "{helper} announces outward at {:?}",
            preview.outward
        );
        let changed = preview.changed();
        eprintln!("{helper}: changes {changed:?}");
        let before = self.job_ids(&devices).await?;
        let job = self.write(helper, &preview.fingerprint).await?;
        let written = Instant::now();
        let checked = async {
            self.rolled_out(written, Some(&changed)).await?;
            let applied = self.applied(&devices, &before).await?;
            let expected = preview
                .scripts
                .iter()
                .filter(|(_, script)| !script.is_empty())
                .map(|(name, script)| (name.clone(), script.clone()))
                .collect::<BTreeMap<_, _>>();
            ensure!(
                applied == expected,
                "{helper}: applied:\n{applied:#?}\npreviewed:\n{expected:#?}"
            );
            self.all_like_netbox(&devices).await
        }
        .await;
        Ok(Written {
            write: Write { job, changed },
            checked,
        })
    }

    /// Undoes `write` and waits for the rollout to bring its devices back; every device like
    /// NetBox after. Where its check failed (`checked`), which devices got it is unknown.
    async fn take_back(&self, write: &Write, checked: bool) -> anyhow::Result<()> {
        self.undo(write.job).await?;
        self.rolled_out(Instant::now(), checked.then_some(&write.changed))
            .await?;
        self.all_like_netbox(&self.routeros_devices().await?).await
    }

    /// Writes `helper`'s plan, checks the rollout against the preview and takes it back, with
    /// the plan as before.
    async fn try_helper(&self, helper: &str) -> anyhow::Result<()> {
        let (fingerprint, _) = self.plan_fingerprint(helper).await?;
        let written = self.write_checked(helper).await?;
        let checked = written.checked.is_ok();
        let back = self.take_back(&written.write, checked).await;
        written.checked?;
        back?;
        self.same_plan(helper, &fingerprint).await
    }

    /// Writes `helper`'s plan, then `removal`'s taking it out again, each checked like
    /// [`Self::write_checked`]; the lab is as before after, `helper` plans as before. What is
    /// written is taken back where a check fails.
    async fn try_taking_out(&self, helper: &str, removal: &str) -> anyhow::Result<()> {
        let (fingerprint, _) = self.plan_fingerprint(helper).await?;
        let written = self.write_checked(helper).await?;
        let result = match written.checked {
            Ok(()) => match self.write_checked(removal).await {
                Ok(removed) => match removed.checked {
                    Ok(()) => self.same_plan(helper, &fingerprint).await,
                    Err(error) => {
                        self.take_back(&removed.write, false).await?;
                        Err(error)
                    }
                },
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        if result.is_err() {
            self.take_back(&written.write, false).await?;
        }
        result
    }

    async fn same_plan(&self, helper: &str, fingerprint: &str) -> anyhow::Result<()> {
        let (again, _) = self.plan_fingerprint(helper).await?;
        ensure!(again == fingerprint, "{helper}: another plan than before");
        Ok(())
    }
}

/// A plan written to NetBox by [`Lab::write_checked`].
struct Written {
    write: Write,
    /// what the check after the rollout found
    checked: anyhow::Result<()>,
}

/// What [`Lab::take_back`] undoes.
struct Write {
    job: u32,
    /// the devices its preview changes
    changed: BTreeSet<String>,
}

/// `fields` as a form body.
fn form(fields: &[(&str, &str)]) -> String {
    let encoded = |text: &str| {
        text.bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    char::from(byte).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect::<String>()
    };
    fields
        .iter()
        .map(|(key, value)| format!("{}={}", encoded(key), encoded(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// `script` trimmed, its keys left out: a plan makes a new PSK each time it is computed.
fn masked(script: &str) -> String {
    let word = |word: &str| match word.split_once('=') {
        Some((key, _))
            if [
                "auth-key",
                "passphrase",
                "psk",
                "password",
                "wpa2-pre-shared-key",
            ]
            .contains(&key) =>
        {
            format!("{key}=***")
        }
        _ => word.into(),
    };
    script
        .trim()
        .lines()
        .map(|line| line.split(' ').map(word).collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// „Loopback“ for the lab's switch without RouterOS: `lo`, its addresses as primary IPs, no
/// device's configuration changing.
#[tokio::test]
#[ignore]
async fn loopback() {
    let lab = Lab::from_env();
    let switch = lab
        .device_id("lab-keller-switch")
        .await
        .expect("the switch");
    lab.try_helper(&format!("{{ loopback: {switch} }}"))
        .await
        .unwrap();
}

/// „Verbindung“ between two free ports of lab-hex and lab-crs226 (no cable at either: OSPF on
/// them meets nobody), undone; then written again and taken out with „Abräumen“ (the
/// helper `disconnect`), which leaves the lab as before. Not lab-np2's: as the management
/// proxy it rolls out by hand, a change to it holds the rollout back.
#[tokio::test]
#[ignore]
async fn connection_and_disconnect() {
    let lab = Lab::from_env();
    let hex = lab.device_id("lab-hex").await.expect("lab-hex");
    let crs226 = lab.device_id("lab-crs226").await.expect("lab-crs226");
    let port = lab.interface_id(hex, "ether5").await.expect("ether5");
    let peer = lab.interface_id(crs226, "ether24").await.expect("ether24");
    let connection = format!("{{ connection: {{ interface: {port}, peer: {peer} }} }}");
    lab.try_helper(&connection).await.unwrap();
    let disconnect = format!("{{ disconnect: {{ device: {hex}, interface: {port} }} }}");
    lab.try_taking_out(&connection, &disconnect).await.unwrap();
}

/// „Kunden-Netz“ with DHCP on a free port of lab-crs226 (no cable), from the lab's prefixes,
/// undone; then written again and taken out with „Kunden-Port abräumen“.
#[tokio::test]
#[ignore]
async fn customer_net_and_disconnect() {
    let lab = Lab::from_env();
    let crs226 = lab.device_id("lab-crs226").await.expect("lab-crs226");
    let port = lab.interface_id(crs226, "ether24").await.expect("ether24");
    let (parent, parent_v6) = (
        lab.prefix_id("192.168.102.0/23")
            .await
            .expect("the IPv4 prefix"),
        lab.prefix_id("2a02:16a:fe00:9000::/56")
            .await
            .expect("the IPv6 prefix"),
    );
    let net = format!(
        "{{ customerNet: {{ device: {crs226}, target: {{ interface: {port} }}, parent: {parent}, parentV6: {parent_v6}, dhcp: true }} }}"
    );
    lab.try_helper(&net).await.unwrap();
    let disconnect = format!("{{ customerDisconnect: {{ device: {crs226}, interface: {port} }} }}");
    lab.try_taking_out(&net, &disconnect).await.unwrap();
}
