//! A pane's life, from its id appearing in a workspace to its going: the
//! process half of TerminalPane.tsx (inventory §4/1.5, "Mount sequence",
//! "Unmount", and §4/3, "PaneController").
//!
//! A pane's identity is its id. A restart, a change of agent or of shell
//! gives the pane a new id upstream (`respawnPane`), which unmounts the old
//! one (its shell killed) and mounts the new one (a new shell), as the React
//! effect keyed on `pane.id` did. Mounts and unmounts follow the model:
//! whatever changes a workspace's panes, the next sync starts what appeared
//! and stops what went (mod.rs, `sync`).
//!
//! Where the agent stands (its activity, the mod's reports, hand-backs and
//! their notifications, the queue, the session to resume) is P4's
//! (`panes_logic`); this file tells it what happens to the pane, in the
//! order its contract asks: `Mounted` before the PTY spawns, `Status` once
//! the shell runs or could not start, `WatchSession` once the agent is
//! launched, `Unmounted` when the terminal goes.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hangar_core::kits::{KitInfo, LaunchIssue};
use hangar_core::pty::PaneJob;
use hangar_i18n::{Key, Locale, Translator, args};
use hangar_term::Terminal;
use iced::Task;

use crate::accounts::{Follow, FollowInput, Hold, Job, Turn, follow};
use crate::kits::model::{
    KitLaunchIssue, LaunchReason, PaneKits, kit_issue_notes, load_pane_kits_with,
};
use crate::mcp::scope::{ShellFamily, mcp_config_arg, shell_family, workspace_mcp};
use crate::message::{Intent, Message};
use crate::orchestration_ui::actions::{
    RestartText, RoleRestart, RoleRestartInput, is_mid_turn, queue_restart_note, restart_text,
    role_restart, take_restart_note,
};
use crate::panes_logic::Msg as Logic;
use crate::panes_logic::agents::{LaunchOptions, is_resumable, known_session_ids, launch_command};
use crate::panes_logic::shells::resolve_shell;
use crate::services::{CoreEvent, Services};
use crate::state::live::{PaneEventDetail, PaneMcp, PaneMcpMode, PaneStatus};
use crate::state::roles::{ORCHESTRATION_ALLOWED_TOOLS, orchestration_of};
use crate::state::types::{AgentId, McpMode, Pane, PaneRole, Settings, ShellInfo, Workspace};
use crate::state::{Model, PaneId, Patch, StoreAction, WsId};

use super::Msg;
use super::hub::HubChannel as Channel;
use super::hub::{Hub, HubChannel};
use super::launch::{self, Check, Settle};
use super::look::{self, Look};
use super::pty_link::{PaneLink, PtyBackend};
use super::search::Search;
use super::waves;
use crate::nodes::remote::{self, RemoteMcp, RemoteSpawn, Started};

/// How long a pane waits for its terminal to measure itself before its
/// shell starts at the default 80×24: a pane of a workspace off screen is
/// never measured, as a hidden xterm.js was never fitted.
pub const MEASURE_WAIT: Duration = Duration::from_millis(250);

/// The size a terminal starts at when nothing is going to measure it (its
/// workspace is the chat view): wide and tall enough for Claude Code to draw
/// a long question whole. At 80×24 its dialog scrolled, and an answer typed
/// into it, checked on the screen, was refused.
pub const UNSEEN_SIZE: (u16, u16) = (160, 50);

/// The size a pane's terminal starts at: 80×24 until its first measure, or
/// [`UNSEEN_SIZE`] when no measure is coming.
fn start_size(measured: bool) -> (u16, u16) {
    if measured { (80, 24) } else { UNSEEN_SIZE }
}

/// A pane, while it is mounted.
pub struct PaneRt {
    pub ws: WsId,
    pub name: String,
    pub agent: AgentId,
    /// Its node runs the pane itself (`Workspace::hosted`, as it stood at
    /// the mount): this terminal follows a shell it neither started nor
    /// ends.
    pub hosted: bool,
    pub link: Arc<PaneLink>,
    /// The shell's life as this package lives it; P4's copy is what the
    /// glyphs read.
    pub status: PaneStatus,
    /// What the launch had to say: kept out of the terminal, which Claude
    /// Code clears as it starts.
    pub notes: Vec<String>,
    pub dismissed: HashSet<String>,
    pub search: Option<Search>,
    /// What the search bar held when it last closed.
    pub last_query: String,
    /// The account its Claude Code runs on, as core started it.
    pub account: Option<String>,
    /// The MCP servers its Claude Code started with.
    pub mcp: Option<PaneMcp>,
    /// The role the pane was launched with: a role then meant Hangar's
    /// server came along.
    pub launched_role: Option<PaneRole>,
    /// The restart a role asked for was dealt with, once per pin.
    pub role_handled: bool,
    /// The account a move was started for (`accounts::follow`), handed to
    /// the pane the move's restart makes (`Intent::PaneRespawned`): a launch
    /// core had to put on another account is not restarted again.
    pub moved_to: Option<String>,
    /// Its launch is a move to another account: its conversation is resumed
    /// whatever `auto_resume` says, and its note says where it landed.
    pub moved: bool,
    /// When the person last put something in its prompt between two turns
    /// (typed, pasted, dropped, or dictated without sending it): a restart
    /// would lose it. Cleared by the next turn, which means it was sent.
    pub drafted_at: Option<Instant>,
    /// Where its move to another account stands between two looks.
    pub follow: FollowRt,
    /// The spawn, while the terminal has not measured itself yet or the
    /// machine's shells are not known yet.
    pending: Option<Spawn>,
    /// The spawn was due while the shells were not known: it goes as soon
    /// as they are.
    start_due: bool,
    /// The resume the launch is waiting to type, and whether the hibernation
    /// owed it.
    resume: Option<String>,
    owed: bool,
    /// The `--mcp-config` argument the launch types, and whether it replaces
    /// Claude Code's own servers.
    mcp_arg: Option<(String, bool)>,
    /// The look last applied, to restyle only on a change.
    pub look: Look,
    /// Whether the terminal was last told it has the keyboard.
    pub focused: bool,
    /// The launch's wait for the shell, then its check that the command
    /// ran, aborted with the pane.
    launch: Option<iced::task::Handle>,
    /// The search for its shell on the node that runs it, which goes on
    /// until the shell is found: ended with the pane.
    attach: Option<iced::task::Handle>,
    /// Where its node's story stood when the pane was mounted
    /// (`nodes::Live::reattach`): a pane its node runs whose shell ended is
    /// mounted afresh once the node has reconnected or changed its state
    /// since, in case the node started it again.
    pub seen: u64,
    /// The agent's command as it was typed, for the check to type again.
    typed: Option<String>,
    /// The launch put off while its resume waits its wave (`waves`): what
    /// the shell's readiness answered.
    parked: Option<(Option<bool>, HashSet<String>)>,
}

impl PaneRt {
    /// Whether its shell waits for the machine's shells to be known.
    pub fn start_due(&self) -> bool {
        self.start_due && self.pending.is_some()
    }

    /// Whether the launch `launch_ready` would make with `exists` resumes a
    /// session: the launches that may wait their wave.
    pub fn resumes(&self, exists: Option<bool>) -> bool {
        self.resume.is_some() && exists != Some(false)
    }

    /// Whether the agent's command is still to come: the shell not started,
    /// or not settled yet. A pane on screen in that state holds the panes
    /// off screen back, so that it resumes first.
    pub fn launch_to_come(&self) -> bool {
        self.agent != AgentId::Shell
            && self.parked.is_none()
            && (self.pending.is_some()
                || self.status == PaneStatus::Starting
                || (self.launch.is_some() && self.typed.is_none()))
    }

    /// Whether the command typed was checked: it runs, or was lost.
    pub fn launch_checked(&self) -> bool {
        self.typed.is_none()
    }

    /// Where the resume typed stands, for the waves behind it
    /// (`waves::settling`).
    pub fn settling(&self) -> waves::Settling {
        let quiet = self.link.last_output().map(|at| at.elapsed());
        waves::settling(self.launch_checked(), self.link.program_printed(), quiet)
    }

    /// The launch waits its wave, with what the shell's readiness answered.
    pub fn park(&mut self, exists: Option<bool>, known: HashSet<String>) {
        self.parked = Some((exists, known));
    }

    /// The launch put off, now that its turn came.
    pub fn unpark(&mut self) -> Option<(Option<bool>, HashSet<String>)> {
        self.parked.take()
    }
}

impl std::fmt::Debug for PaneRt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaneRt")
            .field("ws", &self.ws)
            .field("name", &self.name)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// What `pty_spawn` answered, and what the launch learnt on the way.
#[derive(Debug, Clone)]
pub struct SpawnReport {
    /// Why each kit the pane was given stays out of it: the ones held for
    /// what they run by themselves first, then those `pty_spawn` kept out.
    pub kit_issues: Vec<KitLaunchIssue>,
    /// The account the pane was launched on, as P8's choice resolved it.
    pub resolved: Option<String>,
    /// The account that was asked for and is gone: the fallback applies.
    pub missing: Option<String>,
    /// The account core started its Claude Code on (main when it fell back).
    pub account: Option<String>,
    pub mcp: Option<PaneMcp>,
    pub mcp_arg: Option<(String, bool)>,
    /// The MCP scope's warnings, already worded.
    pub notes: Vec<String>,
    /// A pane of a remote workspace: how its shell started on the node.
    pub remote: Option<RemoteStart>,
}

/// How a remote pane's shell started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteStart {
    /// It was running already (the desktop restarted, the workspace was
    /// opened again): nothing is launched in it, and its history up to
    /// `replay_until` is drawn as it was.
    pub attached: bool,
    pub replay_until: u64,
    /// The node runs the pane itself: its shell is never this machine's to
    /// end.
    pub hosted: bool,
}

/// The shell a pane runs: its own, its workspace's, the settings', then the
/// first detected (`resolveShell`).
pub fn shell_of<'a>(m: &'a Model, ws: &Workspace, pane: &Pane) -> Option<&'a ShellInfo> {
    resolve_shell(
        &m.ui.shells,
        &[
            pane.shell_id.as_deref(),
            ws.shell_id.as_deref(),
            m.settings().shell_id.as_deref(),
        ],
    )
}

/// The folder a pane's shell opens in: P4's rule, the one every package uses.
pub use crate::panes_logic::folder_of;

/// What a pane's kits are decided from: its workspace's entries and the ones
/// turned on everywhere, with their grants, as the model held them when the
/// pane mounted. Only these two lists are kept, not the whole workspace and
/// settings.
#[derive(Debug, Clone, Default)]
struct KitChoice {
    ws: Workspace,
    settings: Settings,
}

impl KitChoice {
    fn of(settings: &Settings, ws: &Workspace) -> KitChoice {
        KitChoice {
            ws: Workspace {
                kits: ws.kits.clone(),
                ..Workspace::default()
            },
            settings: Settings {
                kits_everywhere: settings.kits_everywhere.clone(),
                ..Settings::default()
            },
        }
    }
}

/// The kits a pane starts with, and the ones held out (`loadPaneKits`), from
/// `list`, the kits on disk as they are at the spawn. A GitHub kit whose
/// hooks (or anything else it runs by itself) changed since the workspace
/// accepted them stays out of the pane until they are accepted again: core's
/// `pty_spawn` loads every id it is handed, so this is where the consent is
/// held. What else keeps a kit out (missing, broken, forbidden by a policy)
/// core says at spawn. `None`, a login pane, starts with no kit at all.
async fn launch_kits<F, Fut>(choice: Option<&KitChoice>, list: F) -> PaneKits
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Vec<KitInfo>, String>>,
{
    match choice {
        Some(choice) => load_pane_kits_with(Some(&choice.ws), Some(&choice.settings), list).await,
        None => PaneKits::default(),
    }
}

/// Why each kit a pane was given stays out of it, in the order its notes are
/// shown (`[...held, ...kitIssues]`): those held for what they run by
/// themselves, then what `pty_spawn` kept out.
fn kit_issues(held: &[String], spawned: &[LaunchIssue]) -> Vec<KitLaunchIssue> {
    held.iter()
        .map(|id| KitLaunchIssue {
            id: id.clone(),
            reason: LaunchReason::Hooks,
            detail: None,
        })
        .chain(spawned.iter().map(KitLaunchIssue::from))
        .collect()
}

/// `--permission-mode` and `--allowedTools` of a launch: for an orchestrator
/// and its workers, which work while nobody watches, the mode the workspace
/// chose for them, read as `orchestrationOf` reads it (a mode that is not
/// offered, edited in by hand, is the person's own default), and Hangar's
/// own tools; nothing for any other pane.
fn role_flags(ws: &Workspace, pane: &Pane) -> (Option<String>, Vec<String>) {
    if pane.role.is_none() {
        return (None, Vec::new());
    }
    let tools = ORCHESTRATION_ALLOWED_TOOLS
        .iter()
        .map(|tool| (*tool).to_owned())
        .collect();
    (orchestration_of(ws).permission_mode, tools)
}

/// The Claude account choices of a launch, the settings', the workspace's
/// and the pane's, which P8's `launch_account` resolves (pane, workspace,
/// default, then main) off the UI thread: it reads the accounts' files.
#[derive(Debug, Clone, Default)]
struct AccountChoice {
    default: Option<String>,
    workspace: Option<String>,
    pane: Option<String>,
}

/// What `pty_spawn` is called with, gathered on the UI thread.
struct Spawn {
    id: PaneId,
    workspace_id: WsId,
    workspace_cwd: String,
    cwd: String,
    shell: Option<ShellInfo>,
    name: String,
    bridge: bool,
    port_base: Option<u16>,
    /// The workspace's build cache variables, for a worker (it has a
    /// `port_base`) of a workspace that keeps the cache on.
    build_env: Vec<(String, String)>,
    /// `None` for a login pane, which starts with no kit.
    kits: Option<KitChoice>,
    agent: AgentId,
    /// A Claude Code pane's: the account it is launched on.
    account: Option<AccountChoice>,
    /// The workspace's MCP choice applies (a Claude Code that is not a login
    /// pane).
    mcp: Option<McpLaunch>,
    /// `pane-scopes` is on: the shell goes into a systemd scope of its own.
    scoped: bool,
    /// The node of a remote workspace, or the pane's own: the shell starts
    /// there.
    node: Option<String>,
    /// A worker a local workspace sent to a node: its board is this
    /// machine's (`remote::BOARD_RELAY`).
    away: bool,
    /// A pane its node runs itself: found there, never started from here.
    hosted: bool,
}

/// What the MCP scope of a launch is made from (`mcpLaunchConfig`).
struct McpLaunch {
    mode: McpMode,
    enabled: Vec<String>,
    /// An orchestrator or a worker gets Hangar's server even in `inherit`:
    /// the board, `worker_*` and `delivery_submit` are its work.
    role: bool,
    family: Option<ShellFamily>,
    tr: Translator,
}

/// What `pane` of `ws` reaches its shell through: core's PTY manager `pty`,
/// or its node — its own (a worker sent to a node from a local workspace),
/// else the one the workspace lives on.
pub fn backend_for(
    ws: &Workspace,
    pane: &Pane,
    pty: &Arc<hangar_core::pty::PtyManager>,
) -> PtyBackend {
    match crate::nodes::away::node_of(ws, pane) {
        Some(node) => PtyBackend::Remote {
            node,
            hosted: ws.is_hosted(),
        },
        None => PtyBackend::Local(pty.clone()),
    }
}

/// Mounts `pane`: its terminal and its writer. The shell starts once the
/// terminal has measured itself (`MEASURE_WAIT` at most), so that the agent
/// draws its first screen at the size it will be seen at; when nothing is
/// going to measure it (`measured` false: its workspace is the chat view),
/// it starts right away, once P4 has heard of the pane.
pub fn mount(
    hub: &Hub,
    sv: &Services,
    m: &Model,
    ws: &Workspace,
    pane: &Pane,
    measured: bool,
) -> (PaneRt, Task<Message>) {
    let settings = m.settings();
    let look = look::look(settings, ws, pane.agent);
    let (columns, rows) = start_size(measured);
    let mut terminal = Terminal::new(look.config.clone(), columns, rows);
    terminal.set_palette(look.palette.clone());
    terminal.set_font(look.font.clone());
    let link = PaneLink::new(
        pane.id.clone(),
        terminal,
        backend_for(ws, pane, sv.core().pty()),
    );
    hub.insert(link.clone());

    let node = crate::nodes::away::node_of(ws, pane);
    let remote = node.is_some();
    let login = pane.agent == AgentId::Claude && pane.login == Some(true);
    let account = (pane.agent == AgentId::Claude).then(|| AccountChoice {
        default: settings.claude_account.clone(),
        workspace: ws.claude_account.clone(),
        pane: pane.claude_account.clone(),
    });
    // Resolved again when the shell starts (`start`), once the machine's
    // shells are known.
    let shell = shell_of(m, ws, pane).cloned();
    let scope = workspace_mcp(Some(ws));
    let spawn = Spawn {
        id: pane.id.clone(),
        workspace_id: ws.id.clone(),
        workspace_cwd: ws.cwd.clone(),
        cwd: folder_of(ws, pane),
        shell: shell.clone(),
        name: pane.name.clone(),
        bridge: settings.claude_bridge,
        port_base: pane.port_base.and_then(|port| u16::try_from(port).ok()),
        build_env: if pane.port_base.is_some() && !remote {
            hangar_core::build_cache::env_for(
                std::path::Path::new(&ws.cwd),
                &ws.id,
                orchestration_of(ws).shared_build_cache,
            )
        } else {
            Vec::new()
        },
        // A pane opened to log its Claude account in runs the login and
        // nothing else: no kits, no servers, no conversation. The TS said so
        // too, but still handed such a pane the kits turned on everywhere
        // (`loadPaneKits(null, settings)`); the comment is followed here.
        // Every other pane gets the kits, a shell pane too: a `claude` typed
        // into it loads them, like the bridge.
        // The kits and the Claude accounts are this machine's files: a pane
        // on a node starts without them (kits of remote files come later).
        kits: (!login && !remote).then(|| KitChoice::of(settings, ws)),
        agent: pane.agent,
        account: account.filter(|_| !remote),
        mcp: (pane.agent == AgentId::Claude && !login).then(|| McpLaunch {
            mode: scope.mode,
            enabled: scope.enabled,
            role: pane.role.is_some(),
            family: shell_family(shell.as_ref()),
            tr: Translator::new(m.locale()),
        }),
        scoped: !remote
            && crate::settings::experiments::enabled(
                settings,
                crate::settings::experiments::PANE_SCOPES,
            ),
        node,
        away: crate::nodes::away::pane_node(pane).is_some(),
        hosted: ws.is_hosted(),
    };

    let task = mounted_then_due(pane.id.clone(), measured);

    let rt = PaneRt {
        ws: ws.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent,
        hosted: ws.is_hosted(),
        link,
        status: PaneStatus::Starting,
        notes: Vec::new(),
        dismissed: HashSet::new(),
        search: None,
        last_query: String::new(),
        account: None,
        mcp: None,
        launched_role: pane.role,
        role_handled: false,
        moved_to: None,
        moved: false,
        drafted_at: None,
        follow: FollowRt::default(),
        pending: Some(spawn),
        start_due: false,
        resume: None,
        owed: false,
        mcp_arg: None,
        look,
        focused: false,
        launch: None,
        attach: None,
        seen: reattach_mark(m, ws),
        typed: None,
        parked: None,
    };
    (rt, task)
}

/// What a mount sends: `Mounted`, then `SpawnDue` once `MEASURE_WAIT` is
/// over, or, when nothing will measure the terminal (`measured` false), right
/// after `Mounted`.
fn mounted_then_due(id: PaneId, measured: bool) -> Task<Message> {
    // P4 hears of the pane before its shell exists, so no report of the mod
    // is lost; the queue a restart carried over goes back to core there.
    let mounted = Task::done(Message::PanesLogic(Logic::Mounted { pane: id.clone() }));
    if measured {
        let due = Task::perform(tokio::time::sleep(MEASURE_WAIT), move |()| {
            Message::Panes(Msg::SpawnDue(id.clone()))
        });
        Task::batch([mounted, due])
    } else {
        // Chained, not batched: `Mounted` is handled before the spawn starts.
        mounted.chain(Task::done(Message::Panes(Msg::SpawnDue(id))))
    }
}

/// Starts the shell, if it has not started: on the terminal's first
/// measure, or once `MEASURE_WAIT` is over, and never before the machine's
/// shells are known (`shells_known`): the pane's shell is resolved here,
/// and one resolved before detection answered would be the first shell
/// found (the login shell) instead of the pane's. The kits on disk are read first,
/// to hold out those whose hooks were not accepted; the spawn then runs on
/// the blocking pool, since it waits up to 5 s for a shell of the same id
/// still being reaped, and the MCP scope file is written before it.
pub fn start(
    rt: &mut PaneRt,
    m: &Model,
    sv: &Services,
    channel: &HubChannel,
    shells_known: bool,
) -> Task<Message> {
    if rt.pending.is_none() {
        return Task::none();
    }
    if !shells_known {
        rt.start_due = true;
        return Task::none();
    }
    rt.start_due = false;
    let Some(mut spawn) = rt.pending.take() else {
        return Task::none();
    };
    let (columns, rows) = rt.link.lock().size();
    if let Some(node) = spawn.node.clone() {
        let id = spawn.id.clone();
        let label = crate::nodes::label(m, &node);
        if spawn.hosted {
            let (looking, handle) =
                Task::perform(attach_hosted(node, label, id.clone()), move |result| {
                    Message::Panes(Msg::Spawned(id.clone(), result))
                })
                .abortable();
            rt.attach = Some(handle.abort_on_drop());
            return looking;
        }
        let asked = spawn.remote(columns, rows);
        let tr = Translator::new(m.locale());
        return Task::perform(start_remote(node, label, asked, tr), move |result| {
            Message::Panes(Msg::Spawned(id.clone(), result))
        });
    }
    spawn.resolve_shell(m, &rt.ws);
    let sink = rt.link.sink(channel.waker());
    let pty = sv.core().pty().clone();
    let host = sv.core().host().clone();
    let id = spawn.id.clone();
    Task::perform(
        async move {
            let kits = launch_kits(spawn.kits.as_ref(), hangar_core::kits::kits_list).await;
            tokio::task::spawn_blocking(move || spawn.run(&host, &pty, sink, columns, rows, kits))
                .await
                .unwrap_or_else(|error| Err(format!("the spawn stopped: {error}")))
        },
        move |result| Message::Panes(Msg::Spawned(id.clone(), result)),
    )
}

/// How long a pane of a remote workspace waits on its node's publication
/// before looking again (a look is cheap; the wait wakes at the publish).
const NODE_WAIT: Duration = Duration::from_secs(30);

/// Starts the shell of a remote pane on its node, once the node is
/// connected (waiting for it as long as it takes: the pane says meanwhile
/// that its node is away), or finds it still running there.
async fn start_remote(
    node: String,
    label: String,
    spawn: RemoteSpawn,
    tr: Translator,
) -> Result<SpawnReport, String> {
    // Woken as soon as the node is published, not on a polling tick.
    let client = loop {
        if let Some(client) = remote::wait_for(&node, NODE_WAIT).await {
            break client;
        }
    };
    let started = remote::start(&client, &spawn)
        .await
        .map_err(|error| format!("{label}: {error}"))?;
    Ok(remote_report(&spawn, started, tr))
}

/// How far apart a pane its node runs itself is looked for on the node
/// while the node has not started it, or could not be asked.
const HOSTED_EVERY: Duration = Duration::from_millis(500);

/// Finds the shell of a pane its node runs itself, once the node is
/// connected, and follows it: nothing is started, and nothing typed. It
/// is looked for until it is found, for as long as the pane is mounted
/// (the task is the pane's, `PaneRt::attach`): a look that fails, or a
/// node that has not started it yet, is no reason to call it ended and
/// offer a restart that would take down an agent that runs.
async fn attach_hosted(node: String, label: String, pane: PaneId) -> Result<SpawnReport, String> {
    let found = || remote::wait_for(&node, NODE_WAIT);
    let started = remote::attach_for_good(&pane, HOSTED_EVERY, found).await;
    let Started::Attached { seq } = started else {
        return Err(format!("{label}: its node has not started this pane"));
    };
    Ok(SpawnReport {
        kit_issues: Vec::new(),
        resolved: None,
        missing: None,
        account: None,
        mcp: None,
        mcp_arg: None,
        notes: Vec::new(),
        remote: Some(RemoteStart {
            attached: true,
            replay_until: seq,
            hosted: true,
        }),
    })
}

/// What a remote start says, as a local spawn says it.
fn remote_report(spawn: &RemoteSpawn, started: Started, tr: Translator) -> SpawnReport {
    let mut report = SpawnReport {
        kit_issues: Vec::new(),
        resolved: None,
        missing: None,
        account: None,
        mcp: None,
        mcp_arg: None,
        notes: Vec::new(),
        remote: None,
    };
    match started {
        Started::Attached { seq } => {
            report.remote = Some(RemoteStart {
                attached: true,
                replay_until: seq,
                hosted: false,
            });
        }
        Started::Spawned { mcp } => {
            report.remote = Some(RemoteStart {
                attached: false,
                replay_until: 0,
                hosted: false,
            });
            let Some(asked) = &spawn.mcp else {
                // Claude Code with its own servers, as an `inherit` launch.
                if spawn.agent == AgentId::Claude.as_str() {
                    report.mcp = Some(PaneMcp {
                        mode: PaneMcpMode::Inherit,
                        hangar: Some(false),
                    });
                }
                return report;
            };
            let Some(file) = mcp.filter(|file| !file.path.is_empty()) else {
                return report;
            };
            if !file.missing.is_empty() {
                report.notes.push(
                    tr.t_with(
                        Key::McpScopeWarnMissing,
                        &args!("names" => file.missing.join(", ")),
                    )
                    .into_owned(),
                );
            }
            // The node runs a POSIX shell: the variable its spawn sets,
            // never the path, is what the command names.
            report.mcp_arg =
                mcp_config_arg(&file.path, Some(ShellFamily::Posix)).map(|arg| (arg, asked.strict));
            report.mcp = Some(PaneMcp {
                mode: if asked.strict {
                    PaneMcpMode::Custom {
                        servers: file.servers.clone(),
                    }
                } else {
                    PaneMcpMode::Inherit
                },
                hangar: Some(report.mcp_arg.is_some()),
            });
        }
    }
    report
}

impl Spawn {
    /// What the node is asked for: the parameters that mean something
    /// there, the MCP scope as the workspace chose it (`mcp_scope`'s rules:
    /// nothing for an `inherit` pane without a role).
    fn remote(&self, columns: u16, rows: u16) -> RemoteSpawn {
        let mcp = self.mcp.as_ref().and_then(|launch| {
            let inherit = launch.mode == McpMode::Inherit;
            if inherit && !launch.role {
                return None;
            }
            Some(RemoteMcp {
                workspace_id: self.workspace_id.clone(),
                workspace_cwd: self.workspace_cwd.clone(),
                enabled: if inherit {
                    Vec::new()
                } else {
                    launch.enabled.clone()
                },
                strict: !inherit,
            })
        });
        RemoteSpawn {
            id: self.id.clone(),
            cwd: self.cwd.clone(),
            cols: columns,
            rows,
            name: self.name.clone(),
            agent: self.agent.as_str().to_owned(),
            bridge: self.bridge,
            port_base: self.port_base,
            mcp,
            env: if self.away {
                vec![(crate::nodes::remote::BOARD_RELAY.to_owned(), "1".to_owned())]
            } else {
                Vec::new()
            },
        }
    }

    /// The pane's shell, resolved against the shells detected by now, and
    /// the quoting of its MCP argument with it.
    fn resolve_shell(&mut self, m: &Model, ws: &str) {
        let Some((ws, pane)) = find(m, ws, &self.id) else {
            return;
        };
        let shell = shell_of(m, ws, pane).cloned();
        if let Some(mcp) = &mut self.mcp {
            mcp.family = shell_family(shell.as_ref());
        }
        self.shell = shell;
    }

    /// `kits` is what [`launch_kits`] found: only its `ids` reach the shell
    /// and the MCP scope, and its `held` become notes.
    fn run(
        self,
        host: &hangar_core::Host,
        pty: &hangar_core::pty::PtyManager,
        sink: hangar_core::pty::PtySink,
        columns: u16,
        rows: u16,
        kits: PaneKits,
    ) -> Result<SpawnReport, String> {
        let mut notes = Vec::new();
        let resolved = self.account.as_ref().map(|choice| {
            crate::accounts::launch_account(
                choice.default.as_deref(),
                choice.workspace.as_deref(),
                choice.pane.as_deref(),
            )
        });
        let account = resolved.as_ref().map(|resolved| resolved.id.clone());
        let missing = resolved.and_then(|resolved| resolved.missing);
        let (mcp, scope) = match &self.mcp {
            Some(launch) => self.mcp_scope(host, launch, &kits.ids, account.as_deref(), &mut notes),
            None => (None, None),
        };
        let spawned = hangar_core::pty::pty_spawn_with(
            pty,
            sink,
            self.id,
            self.cwd,
            self.shell,
            columns,
            rows,
            Some(self.name),
            Some(self.bridge),
            self.port_base,
            self.build_env,
            // The shell is handed the file's path (`HANGAR_MCP_CONFIG`); the
            // typed command names the variable, never the path, which holds
            // the person's name.
            scope.as_ref().map(|(path, _, _)| path.clone()),
            Some(kits.ids),
            Some(self.agent.as_str().to_owned()),
            account.clone(),
            self.scoped,
        )?;
        Ok(SpawnReport {
            kit_issues: kit_issues(&kits.held, &spawned.kit_issues),
            resolved: account,
            missing,
            account: spawned.account,
            mcp,
            mcp_arg: scope.map(|(_, arg, strict)| (arg, strict)),
            notes,
            remote: None,
        })
    }

    /// The `--mcp-config` file of this launch, when the workspace chose its
    /// servers or the pane has a role, and the argument naming it
    /// (`mcpLaunchConfig`). Written again at each launch, from Claude Code's
    /// configuration as it stands. Whatever goes wrong (a WSL pane, a path
    /// the shell cannot be handed, a failed write), the agent still starts
    /// with its own servers, and the pane says so.
    fn mcp_scope(
        &self,
        host: &hangar_core::Host,
        launch: &McpLaunch,
        kits: &[String],
        account: Option<&str>,
        notes: &mut Vec<String>,
    ) -> (Option<PaneMcp>, Option<(String, String, bool)>) {
        let tr = launch.tr;
        let inherit = |hangar| PaneMcp {
            mode: PaneMcpMode::Inherit,
            hangar: Some(hangar),
        };
        let failed = |error: String| {
            tr.t_with(
                Key::McpScopeWarnFailed,
                &args!("error" => error.trim_start_matches("Error: ")),
            )
            .into_owned()
        };
        let write = |enabled: Vec<String>, kits: Option<Vec<String>>| {
            hangar_core::mcp_scope::mcp_scope_write(
                host,
                self.workspace_id.clone(),
                self.workspace_cwd.clone(),
                self.cwd.clone(),
                enabled,
                kits,
                account.map(str::to_owned),
            )
        };
        if launch.mode == McpMode::Inherit {
            if !launch.role || launch.family == Some(ShellFamily::Wsl) {
                return (Some(inherit(false)), None);
            }
            return match write(Vec::new(), None) {
                Ok(file) => match mcp_config_arg(&file.path, launch.family) {
                    Some(arg) => (Some(inherit(true)), Some((file.path, arg, false))),
                    None => (Some(inherit(false)), None),
                },
                Err(error) => {
                    notes.push(failed(error));
                    (Some(inherit(false)), None)
                }
            };
        }
        if launch.family == Some(ShellFamily::Wsl) {
            notes.push(tr.t(Key::McpScopeWarnWsl).into_owned());
            return (Some(inherit(false)), None);
        }
        // The servers of the kits the pane starts with, never of one held
        // out: its servers are among what it runs by itself.
        let file = match write(launch.enabled.clone(), Some(kits.to_vec())) {
            Ok(file) => file,
            Err(error) => {
                notes.push(failed(error));
                return (Some(inherit(false)), None);
            }
        };
        let Some(arg) = mcp_config_arg(&file.path, launch.family) else {
            notes.push(failed(tr.t(Key::McpScopeWarnQuote).into_owned()));
            return (Some(inherit(false)), None);
        };
        if !file.missing.is_empty() {
            notes.push(
                tr.t_with(
                    Key::McpScopeWarnMissing,
                    &args!("names" => file.missing.join(", ")),
                )
                .into_owned(),
            );
        }
        let mcp = PaneMcp {
            mode: PaneMcpMode::Custom {
                servers: file.servers.clone(),
            },
            hangar: Some(true),
        };
        (Some(mcp), Some((file.path, arg, true)))
    }
}

fn status(pane: &PaneId, status: PaneStatus) -> Task<Message> {
    Task::done(Message::PanesLogic(Logic::Status {
        pane: pane.clone(),
        status,
    }))
}

/// The spawn answered.
pub fn spawned(
    rt: &mut PaneRt,
    m: &mut Model,
    channel: &Channel,
    result: Result<SpawnReport, String>,
) -> Task<Message> {
    let tr = Translator::new(m.locale());
    let id = rt.link.id.clone();
    let report = match result {
        Ok(report) => report,
        Err(error) => {
            // A pane that cannot start says so in its terminal and offers its
            // restart button, instead of sitting on "starting".
            let line = format!(
                "\r\n\x1b[31m{}: {error}\x1b[0m\r\n",
                tr.t(Key::PaneSpawnFailed)
            );
            rt.link.lock().feed(line.as_bytes());
            rt.status = PaneStatus::Exited;
            return status(&id, PaneStatus::Exited);
        }
    };
    rt.status = PaneStatus::Running;
    // A remote pane's output is followed from now on: its stream exists.
    if let Some(start) = report.remote {
        rt.link.set_remote_alive(true);
        rt.link.follow_remote(channel.waker(), start.replay_until);
    }
    // The terminal may have been measured again while the shell started.
    let (columns, rows) = rt.link.lock().size();
    rt.link.resize(columns, rows);
    rt.mcp = report.mcp.clone();
    rt.mcp_arg = report.mcp_arg.clone();
    // The inspector's MCP row and the MCP panel's "is it up to date" read the
    // servers a pane started with from P4's read-model, not from here.
    m.live.panes_logic.set_launch_mcp(&id, report.mcp.clone());

    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return status(&id, PaneStatus::Running);
    };
    let (ws, pane) = (ws.clone(), pane.clone());

    // In the pane's bar, not its terminal: Claude Code clears the screen as
    // it starts and the notes would go with it. A restart that gave the
    // pane Hangar's tools says so on the way in.
    let mut notes: Vec<String> = take_restart_note(pane.session_id.as_deref())
        .into_iter()
        .collect();
    // A pane of the old app whose agent this version no longer runs opens as
    // a shell (lenient.rs): said once, as it first starts, and the mark
    // taken off so that a restart or a sleep does not say it again. Taking it
    // off is a change, so the pane is saved as the shell it now is.
    if let Some(agent) = pane.retired_agent {
        notes.push(
            tr.t_with(Key::PaneAgentRetired, &args!("agent" => agent))
                .into_owned(),
        );
        m.apply(StoreAction::UpdatePane {
            ws: ws.id.clone(),
            pane: id.clone(),
            patch: Patch::new(|pane: &mut Pane| pane.retired_agent = None),
        });
    }
    // The account a pane fell back to: the one asked for is gone, or core
    // could not start Claude Code on it.
    let fell_back =
        report.missing.is_some() || (report.account.is_some() && report.account != report.resolved);
    // Not a pane its node runs itself: the account it runs on is the node's.
    if rt.agent == AgentId::Claude && !rt.hosted {
        if let Some(missing) = &report.missing {
            notes.push(
                tr.t_with(Key::AccountsPickerMissing, &args!("id" => missing.as_str()))
                    .into_owned(),
            );
        }
        notes.extend(report.notes.iter().cloned());
        // Said where Claude Code runs; a shell pane has no use for it.
        notes.extend(kit_issue_notes(&report.kit_issues, tr));
        // What the pane's usage counts against until it restarts, whatever
        // is chosen meanwhile (P8's `Launched`).
        let account = report
            .account
            .clone()
            .or(report.resolved.clone())
            .unwrap_or_else(|| crate::accounts::MAIN_ACCOUNT.to_owned());
        m.live.accounts.launched.note(&id, Some(&account));
        rt.account = Some(account);
    }
    rt.notes = notes;
    let running = status(&id, PaneStatus::Running);
    // A shell found running on its node already runs whatever it was given.
    let attached = report.remote.is_some_and(|start| start.attached);
    if rt.agent == AgentId::Shell || attached {
        return running;
    }

    let delay = Duration::from_millis(m.settings().launch_delay_ms);
    let auto_resume = m.settings().auto_resume;

    // The person logs in in the browser; nothing to resume or follow. Never
    // on an account the pane fell back to: logging in to an account that is
    // gone would log main in again in its place.
    if pane.login == Some(true) && rt.agent == AgentId::Claude {
        if fell_back {
            return running;
        }
        let (task, handle) = Task::perform(shell_ready(rt.link.clone(), delay), move |()| {
            Message::Panes(Msg::LoginReady(id.clone()))
        })
        .abortable();
        rt.launch = Some(handle.abort_on_drop());
        return Task::batch([running, task]);
    }

    // A pane its workspace's sleep stopped resumes whatever the setting
    // says: a sleep stops an agent to free memory, and must not cost a
    // conversation.
    let owed = m.live.panes_logic.must_resume(&id, false);
    // A move to another account keeps the conversation whatever the setting
    // says: the person chose an account, not a new conversation.
    let saved = (auto_resume || owed || rt.moved)
        .then(|| pane.session_id.clone())
        .flatten()
        .filter(|_| is_resumable(rt.agent));
    rt.resume = saved.clone();
    rt.owed = owed;
    let agent = rt.agent;
    let cwd = folder_of(&ws, &pane);
    // Claude Code names a session the moment it starts but writes its
    // transcript only on the first message, so a pane reset and left alone
    // holds an id with nothing behind it, and resuming that only prints "No
    // conversation found". Asked while the shell settles; the transcripts
    // known before the launch are read then too, so a brand new one stands
    // out to the session hunt.
    let link = rt.link.clone();
    // The transcripts of a remote pane are the node's: none is looked for
    // here, and its sessions are not followed (the chat view of a remote
    // workspace comes later).
    let local = link.node().is_none();
    let (task, handle) = Task::perform(
        async move {
            let exists = async {
                match saved.filter(|_| local) {
                    Some(id) => transcript_exists(id).await,
                    None => None,
                }
            };
            let known = async move {
                if local {
                    known_session_ids(agent, cwd).await
                } else {
                    HashSet::new()
                }
            };
            let (exists, known, ()) = iced::futures::join!(exists, known, shell_ready(link, delay));
            (exists, known)
        },
        move |(exists, known)| Message::Panes(Msg::LaunchReady(id.clone(), exists, known)),
    )
    .abortable();
    rt.launch = Some(handle.abort_on_drop());
    Task::batch([running, task])
}

/// Resolves once the pane's new shell is ready for a command: it printed
/// something and paused, within the bounds `launch::settle` keeps
/// (`launchDelayMs` at least).
async fn shell_ready(link: Arc<PaneLink>, min: Duration) {
    let started = Instant::now();
    loop {
        let since_output = link.last_output().map(|at| at.elapsed());
        match launch::settle(started.elapsed(), since_output, min) {
            Settle::Type => return,
            Settle::Wait(wait) => tokio::time::sleep(wait.max(Duration::from_millis(1))).await,
        }
    }
}

/// Whether the transcript of `session` is on disk. A "no" is asked again a
/// moment later: at a relaunch the disk is busy, and a "no" costs the pane
/// its resume.
async fn transcript_exists(session: String) -> Option<bool> {
    let ask = |session: String| async move {
        tokio::task::spawn_blocking(move || hangar_core::sessions::session_exists(session))
            .await
            .ok()
            .flatten()
    };
    match ask(session.clone()).await {
        Some(false) => {
            tokio::time::sleep(Duration::from_millis(250)).await;
            ask(session).await
        }
        found => found,
    }
}

/// Waits for the echo of the command just typed (a little longer once it
/// shows, for the program to start), then asks the shell whether it runs
/// something.
fn check_launch(rt: &mut PaneRt, sv: &Services, attempt: u8) -> Task<Message> {
    let link = rt.link.clone();
    let pty = sv.core().pty().clone();
    let id = link.id.clone();
    let (task, handle) = Task::perform(
        async move {
            let deadline = Instant::now() + launch::ECHO_WAIT;
            while Instant::now() < deadline && !link.echoed() {
                tokio::time::sleep(launch::POLL).await;
            }
            if link.echoed() {
                tokio::time::sleep(launch::RUN_WAIT).await;
            }
            // Unreadable is not "nothing runs": the command is not typed
            // into an agent that may have started. A remote shell's children
            // are not asked of its node, which is "unreadable" too.
            if link.node().is_some() {
                return true;
            }
            hangar_core::pty::pty_has_children(pty, vec![link.id.clone()])
                .await
                .map_or(true, |running| !running.is_empty())
        },
        move |running| Message::Panes(Msg::LaunchChecked(id.clone(), attempt, running)),
    )
    .abortable();
    rt.launch = Some(handle.abort_on_drop());
    task
}

/// Types `command` on an emptied line.
fn type_line(rt: &PaneRt, command: &str) {
    rt.link
        .type_command(command, format!("{}{command}\r", launch::CLEAR_LINE));
}

/// What the check of a launch found: nothing to do, the command typed once
/// more, or the loss said.
pub fn launch_checked(rt: &mut PaneRt, sv: &Services, attempt: u8, running: bool) -> Task<Message> {
    rt.launch = None;
    let Some(command) = rt.typed.clone() else {
        return Task::none();
    };
    match launch::check(running, attempt) {
        Check::Ran => {
            rt.typed = None;
            Task::none()
        }
        Check::Retry => {
            eprintln!(
                "pane {}: the agent's command did not start (echoed: {}); typing it again",
                rt.link.id,
                rt.link.echoed()
            );
            type_line(rt, &command);
            check_launch(rt, sv, attempt + 1)
        }
        Check::Lost => {
            rt.typed = None;
            eprintln!(
                "pane {}: the agent's command was typed twice and never started: {command}",
                rt.link.id
            );
            Task::none()
        }
    }
}

/// The shell has settled: type the agent's command, start following its
/// session, and check that the command ran.
pub fn launch_ready(
    rt: &mut PaneRt,
    m: &mut Model,
    sv: &Services,
    exists: Option<bool>,
    known: HashSet<String>,
) -> Task<Message> {
    rt.launch = None;
    let id = rt.link.id.clone();
    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return Task::none();
    };
    let (ws, pane) = (ws.clone(), pane.clone());
    if rt.owed {
        m.live.panes_logic.must_resume(&id, true);
    }
    let mut resume = rt.resume.take();
    // A transcript not found is not resumed, but its id is kept: one miss
    // may be a root not looked in or a read that failed, and forgetting it
    // would lose the conversation for good. A second miss in a row forgets
    // it.
    let forget = resume
        .as_deref()
        .is_some_and(|id| m.live.panes_logic.session_looked_up(id, exists));
    if exists == Some(false) {
        if let Some(id) = &resume {
            eprintln!(
                "pane {}: no transcript for session {id}; starting fresh",
                rt.link.id
            );
        }
        resume = None;
    }
    // Said once it is known whether the conversation came along: a session
    // with no id yet, or whose transcript is not found, starts afresh.
    if rt.moved {
        rt.moved = false;
        let note = moved_note(m, rt.account.as_deref(), resume.is_some());
        rt.notes.push(note);
    }
    let settings = m.settings();
    let (permission_mode, tools) = role_flags(&ws, &pane);
    // Read when the agent starts: a workspace renamed later shows on the
    // next launch.
    let name = crate::state::pane_names::session_name(&pane.name, &ws.name);
    let options = LaunchOptions {
        extra_roots: &ws.extra_roots,
        command: settings.agent_commands.claude.as_deref(),
        name: Some(&name),
        mcp_config: rt.mcp_arg.as_ref().map(|(arg, _)| arg.as_str()),
        mcp_strict: rt.mcp_arg.as_ref().map(|(_, strict)| *strict),
        permission_mode: permission_mode.as_deref(),
        allowed_tools: &tools,
        model: pane.model.as_deref(),
    };
    let mut launched = None;
    let mut check = Task::none();
    if let Some(command) = launch_command(rt.agent, resume.as_deref(), &options) {
        if let Some(id) = &resume {
            m.live.panes_logic.claim_session(id);
        }
        type_line(rt, &command);
        rt.typed = Some(command);
        check = check_launch(rt, sv, 0);
        // Dates the launch for the pane's agent state (P4 ignores it where
        // the bridge or a shell says more).
        launched = Some(Task::done(Message::PanesLogic(Logic::Launched {
            pane: id.clone(),
            resumed: resume.clone(),
        })));
    }
    if forget {
        m.apply(StoreAction::UpdatePane {
            ws: ws.id.clone(),
            pane: id.clone(),
            patch: Patch::new(|pane: &mut Pane| pane.session_id = None),
        });
    }
    // Followed for as long as the pane lives, resumed or not: /new inside
    // the agent opens another transcript, and the pane has to follow it.
    // Not a remote pane's, whose transcripts are on its node.
    let watch = if is_resumable(rt.agent) && rt.link.node().is_none() {
        let mut known = known;
        known.extend(resume);
        Task::done(Message::PanesLogic(Logic::WatchSession {
            pane: id,
            agent: rt.agent,
            cwd: folder_of(&ws, &pane),
            known,
        }))
    } else {
        Task::none()
    };
    match launched {
        Some(launched) => Task::batch([launched, watch, check]),
        None => watch,
    }
}

/// The login pane's shell has settled.
pub fn login_ready(rt: &mut PaneRt) {
    rt.launch = None;
    type_line(rt, crate::accounts::LOGIN_COMMAND);
}

/// The workspace and the pane of `pane`, if both still exist.
pub fn find<'a>(m: &'a Model, ws: &str, pane: &str) -> Option<(&'a Workspace, &'a Pane)> {
    m.find_pane(pane).filter(|(found, _)| found.id == ws)
}

/// Unmounts a pane: its shell killed (off the UI thread: the kill takes
/// core's registry lock, which a blocked write may hold), its launch
/// aborted with it, P4 told.
pub fn unmount(hub: &Hub, sv: &Services, m: &mut Model, rt: PaneRt) -> Task<Message> {
    hub.remove(&rt.link.id);
    let pty = sv.core().pty().clone();
    let id = rt.link.id.clone();
    let node = rt.link.node().map(str::to_owned);
    // A pane its node runs itself is only let go of: the node keeps its
    // shell, whether the pane was closed here, restarted there under another
    // id, or its workspace handed to the node since it was mounted.
    let hosted = rt.hosted || m.find_pane(&id).is_some_and(|(ws, _)| ws.is_hosted());
    drop(rt);
    // Closed, restarted under a new id, or put to sleep: not counted against
    // its account any more.
    m.live.accounts.launched.note(&id, None);
    let unmounted = Task::done(Message::PanesLogic(Logic::Unmounted {
        pane: id.clone(),
        handing_over: false,
    }));
    let kill = match shell_end(node, hosted) {
        ShellEnd::Kept => Task::none(),
        ShellEnd::OnNode(node) => kill_remote(node, id),
        ShellEnd::Local => Task::perform(
            tokio::task::spawn_blocking(move || hangar_core::pty::pty_kill(&pty, id, None)),
            |_| Message::Noop,
        ),
    };
    Task::batch([unmounted, kill])
}

/// Where the story of the node of `ws` stands (`nodes::Live::reattach`); 0
/// for a workspace of this machine.
pub fn reattach_mark(m: &Model, ws: &Workspace) -> u64 {
    ws.node
        .as_ref()
        .and_then(|node| m.live.nodes.reattach.get(node))
        .copied()
        .unwrap_or(0)
}

/// Whether the pane is to be mounted afresh to find its shell again: its
/// node runs it, its shell ended as far as this terminal knows, and the
/// node has reconnected or changed its state since the pane was mounted.
/// The node may have started it again (it does after a restart of its
/// own), which a terminal that stopped at the shell's end would never see.
pub fn stale(rt: &PaneRt, m: &Model) -> bool {
    rt.hosted
        && rt.status == PaneStatus::Exited
        && m.workspace(&rt.ws)
            .is_some_and(|ws| reattach_mark(m, ws) != rt.seen)
}

/// What becomes of the shell of a pane that is unmounted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShellEnd {
    /// Killed here: a pane of this machine.
    Local,
    /// Killed on its node, which this machine started it on.
    OnNode(String),
    /// Left running: its node runs it itself, and only the node ends it.
    Kept,
}

/// What an unmount does with the shell of a pane on `node` (`None`: this
/// machine), `hosted` when its node runs it itself.
pub(crate) fn shell_end(node: Option<String>, hosted: bool) -> ShellEnd {
    match node {
        Some(_) if hosted => ShellEnd::Kept,
        Some(node) => ShellEnd::OnNode(node),
        None => ShellEnd::Local,
    }
}

/// A remote pane's shell killed on its node. A node away keeps it: the
/// shell is found again under its id if the pane comes back, and ends with
/// the node otherwise.
pub fn kill_remote(node: String, pane: PaneId) -> Task<Message> {
    Task::perform(
        async move {
            if let Some(client) = remote::directory(&node)
                && let Err(error) = remote::kill(&client, &pane).await
            {
                eprintln!("pane {pane}: not killed on its node: {error}");
            }
        },
        |()| Message::Noop,
    )
}

/// The shell ended by itself: P4 and the orchestration engine hear of it
/// as core's `pty:exit` (inventory §1.9).
pub fn exited(rt: &mut PaneRt, m: &mut Model, sv: &Services) {
    rt.status = PaneStatus::Exited;
    rt.launch = None;
    m.live.accounts.launched.note(&rt.link.id, None);
    sv.send(Message::Core(CoreEvent::PtyExit {
        pane: rt.link.id.clone(),
    }));
}

/// A Claude Code pinned (or given a role) while it runs, started without
/// Hangar's server, restarts on its own conversation, at once or when its
/// turn ends, so the role comes with the tools (`roleRestart`, P10's).
pub fn role_check(rt: &mut PaneRt, m: &Model, locale: Locale) -> Task<Message> {
    let id = rt.link.id.clone();
    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return Task::none();
    };
    if pane.role.is_none() {
        rt.role_handled = false;
    }
    // A pane its node runs itself is never restarted from here.
    if rt.hosted || rt.status != PaneStatus::Running || rt.role_handled {
        return Task::none();
    }
    let activity = m.live.panes_logic.activity_or_idle(&id);
    let verdict = role_restart(&RoleRestartInput {
        agent: pane.agent,
        role: pane.role,
        launched_role: rt.launched_role,
        hangar: rt.mcp.as_ref().and_then(|mcp| mcp.hangar),
        mid_turn: is_mid_turn(activity),
        session_id: pane.session_id.clone(),
    });
    let say = |rt: &PaneRt, which| {
        let line = format!("\r\n\x1b[33m{}\x1b[0m\r\n", restart_text(locale, which));
        rt.link.lock().feed(line.as_bytes());
    };
    let session = pane.session_id.clone();
    match (verdict, session) {
        (RoleRestart::Skip | RoleRestart::Wait, _) => Task::none(),
        (RoleRestart::Unresumable, _) | (RoleRestart::Now, None) => {
            rt.role_handled = true;
            say(rt, RestartText::Unresumable);
            Task::none()
        }
        (RoleRestart::Now, Some(session)) => {
            rt.role_handled = true;
            say(rt, RestartText::Restarting);
            // The restart gives the pane a new id: the line follows to its
            // terminal.
            queue_restart_note(&session, restart_text(locale, RestartText::Restarting));
            let ws = ws.id.clone();
            Task::perform(
                tokio::task::spawn_blocking(move || {
                    let found = hangar_core::sessions::session_exists(session.clone());
                    (found == Some(true), session)
                }),
                move |answer| {
                    let (found, session) = answer.unwrap_or((false, String::new()));
                    Message::Panes(Msg::RoleRestart(ws.clone(), id.clone(), session, found))
                },
            )
        }
    }
}

/// How long what the person put in a pane's prompt and did not send holds a
/// move back: past it, the draft reads as given up (a prompt cleared with
/// Ctrl+C, a window left with a word typed). Claude Code keeps no draft
/// across a restart, so this errs long; the header says meanwhile that the
/// pane waits.
pub const DRAFT_HOLD: Duration = Duration::from_secs(90);

/// How long a pane waiting to move, whose agent has a command of its own
/// running below it (or whose process table could not be read), waits
/// before it is asked again.
pub const JOB_RECHECK: Duration = Duration::from_secs(15);

/// Where a pane's move to another account stands between two looks
/// (`account_follow`).
#[derive(Debug, Default)]
pub struct FollowRt {
    /// The last look found it waiting to move: its turn, which it waits on,
    /// is part of the panes' signature, so that the turn's end looks again.
    pub pending: bool,
    /// Asked whether a command of its own runs: the answer is on its way.
    checking: bool,
    /// The last answer found one running, or could not tell: not asked
    /// again before this.
    ask_after: Option<Instant>,
    /// What that answer found, which the header says meanwhile.
    busy: Option<Hold>,
    /// The look a timer is set for: a draft that will read as given up, a
    /// command to ask about again.
    wake: Option<Instant>,
}

/// What a look at a pane's account asks of the panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowStep {
    Nothing,
    /// Ask whether a command its agent started still runs below its shell
    /// (`hangar_core::pty::pane_job`, off the UI thread).
    Ask,
    /// Look at it again at this instant (`Msg::FollowWake`).
    Wake(Instant),
    /// Restart it on its conversation: it moves.
    Restart,
}

/// What a look at a pane's account found (`account_follow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowLook {
    pub verdict: Follow,
    /// The account it is about.
    pub to: String,
    /// What the panes are to do.
    pub step: FollowStep,
    /// What holds its move back, which its header says; `None` when there
    /// is no move to wait for.
    pub hold: Option<Hold>,
}

/// Whether the person put something in the pane's prompt since its last
/// turn, recently enough to count as a draft still being written.
pub fn drafting(rt: &PaneRt, now: Instant) -> bool {
    rt.drafted_at
        .is_some_and(|at| now.saturating_duration_since(at) < DRAFT_HOLD)
}

/// The person put something in the pane's prompt (typed, pasted, dropped,
/// or dictated without sending it): between two turns, a draft a restart
/// would lose; inside one, an answer.
pub fn drafted(rt: &mut PaneRt, m: &Model, now: Instant) {
    if !m.live.panes_logic.is_mid_turn(&rt.link.id) {
        rt.drafted_at = Some(now);
    }
}

/// Where the pane's agent stands in its turns, as its hangar-bridge mod
/// reports it: without the mod, its state is a guess read off the screen,
/// which is no ground for a restart.
pub fn turn_of(m: &Model, pane: &str) -> Turn {
    let logic = &m.live.panes_logic;
    match logic.pane_activity(pane) {
        Some(entry) if entry.bridged && logic.is_mid_turn(pane) => Turn::Inside,
        Some(entry) if entry.bridged => Turn::Between,
        _ => Turn::Unknown,
    }
}

/// When the pane's agent last started a turn, as its mod reported it: a
/// different answer later means a turn started (and may have ended) in
/// between.
pub type TurnMark = Option<u64>;

/// The pane's `TurnMark` now: the time of the last turn start its mod
/// reported.
pub fn turn_mark(m: &Model, pane: &str) -> TurnMark {
    m.live
        .panes_logic
        .insight(pane)
        .events
        .iter()
        .find(|event| matches!(event.detail, PaneEventDetail::TurnStart))
        .map(|event| event.at)
}

/// Whether `rt` is a pane an account moves: a running Claude Code of this
/// computer, not opened to log in. Not a pane on a node, whether the node
/// runs it itself, its workspace lives there or it is a worker sent there:
/// its Claude Code starts on the node's login, never on an account of this
/// computer.
pub fn movable(rt: &PaneRt, pane: &Pane) -> bool {
    rt.agent == AgentId::Claude
        && !rt.hosted
        && rt.link.node().is_none()
        && pane.login != Some(true)
        && rt.status == PaneStatus::Running
}

/// The account `pane` of `ws` should run on now (pane > workspace >
/// default), and whether it is logged in (`None` when it is not listed);
/// nothing while the accounts are not known.
pub fn wanted_account(m: &Model, ws: &Workspace, pane: &Pane) -> Option<(String, Option<bool>)> {
    let accounts = m.live.accounts.accounts.as_deref()?;
    let wanted = crate::accounts::resolve_account(
        accounts,
        m.settings().claude_account.as_deref(),
        ws.claude_account.as_deref(),
        pane.claude_account.as_deref(),
    );
    let logged_in = accounts
        .iter()
        .find(|account| account.id == wanted.id)
        .map(|account| account.logged_in);
    Some((wanted.id, logged_in))
}

/// Looks at whether a running Claude Code is to move to the account it
/// should now run on, by itself, when the person does not want to be asked
/// (`Settings::account_switch_ask` off): it moves, on its conversation, once
/// Hangar knows it has nothing to lose (`accounts::follow`). `job` is what
/// is known of the commands its agent started: `Unasked` but from the
/// answer to `FollowStep::Ask` (`job_answered`).
pub fn account_follow(rt: &mut PaneRt, m: &Model, now: Instant, job: Job) -> FollowLook {
    rt.follow.pending = false;
    let stay = FollowLook {
        verdict: Follow::Stay,
        to: String::new(),
        step: FollowStep::Nothing,
        hold: None,
    };
    if m.settings().account_switch_ask {
        return stay;
    }
    let id = rt.link.id.clone();
    let Some((ws, pane)) = find(m, &rt.ws, &id) else {
        return stay;
    };
    let Some((wanted, wanted_logged_in)) = wanted_account(m, ws, pane) else {
        return stay;
    };
    let turn = turn_of(m, &id);
    // A turn begun means what was typed was sent.
    if turn == Turn::Inside {
        rt.drafted_at = None;
    }
    let verdict = follow(&FollowInput {
        movable: movable(rt, pane),
        launched: rt.account.as_deref(),
        wanted: &wanted,
        wanted_logged_in,
        tried: rt.moved_to.as_deref(),
        turn,
        drafting: drafting(rt, now),
        job,
    });
    rt.follow.pending = verdict.pending();
    let step = match verdict {
        Follow::Now => {
            rt.moved_to = Some(wanted.clone());
            FollowStep::Restart
        }
        Follow::Check if rt.follow.checking => FollowStep::Nothing,
        Follow::Check => match rt.follow.ask_after.filter(|at| *at > now) {
            Some(at) => wake(rt, at, now),
            None => {
                rt.follow.checking = true;
                FollowStep::Ask
            }
        },
        Follow::AfterJob => {
            let at = now + JOB_RECHECK;
            rt.follow.ask_after = Some(at);
            wake(rt, at, now)
        }
        Follow::AfterDraft => match rt.drafted_at {
            Some(at) => wake(rt, at + DRAFT_HOLD, now),
            None => FollowStep::Nothing,
        },
        // Its turn's end, or its mod's first report, changes the panes'
        // signature: the sync looks again then. What a turn runs is asked
        // afresh after it.
        Follow::AfterTurn => {
            rt.follow.ask_after = None;
            rt.follow.busy = None;
            FollowStep::Nothing
        }
        Follow::Stay | Follow::NotLoggedIn => FollowStep::Nothing,
    };
    if matches!(verdict, Follow::Now | Follow::Stay | Follow::NotLoggedIn) {
        rt.follow.ask_after = None;
        rt.follow.busy = None;
    }
    let hold = match verdict {
        Follow::Stay | Follow::Now => None,
        Follow::NotLoggedIn => Some(Hold::NotLoggedIn),
        Follow::AfterTurn if turn == Turn::Unknown => Some(Hold::Unreported),
        Follow::AfterTurn => Some(Hold::Turn),
        Follow::AfterDraft => Some(Hold::Draft),
        Follow::Check | Follow::AfterJob => Some(rt.follow.busy.clone().unwrap_or(Hold::Checking)),
    };
    FollowLook {
        verdict,
        to: wanted,
        step,
        hold,
    }
}

/// A look at `at`, unless one is set already for then or before (it looks
/// again, and sets the next one).
fn wake(rt: &mut PaneRt, at: Instant, now: Instant) -> FollowStep {
    if rt.follow.wake.is_some_and(|set| (now..=at).contains(&set)) {
        return FollowStep::Nothing;
    }
    rt.follow.wake = Some(at);
    FollowStep::Wake(at)
}

/// The timer set for `at` went off.
pub fn woke(rt: &mut PaneRt, at: Instant) {
    if rt.follow.wake == Some(at) {
        rt.follow.wake = None;
    }
}

/// The answer to `FollowStep::Ask` came, asked when the agent's last turn
/// had started at `mark`: what it tells of the commands the agent runs, for
/// `account_follow`. A turn started since makes it stale (it may even have
/// ended while the process table was read): it is asked again.
pub fn job_answered(rt: &mut PaneRt, m: &Model, mark: TurnMark, answer: PaneJob) -> Job {
    rt.follow.checking = false;
    if turn_mark(m, &rt.link.id) != mark {
        return Job::Unasked;
    }
    let (job, busy) = match answer {
        PaneJob::Nothing => (Job::Nothing, None),
        PaneJob::Running(what) => (Job::Busy, Some(Hold::Command(what))),
        PaneJob::Unknown => (Job::Busy, Some(Hold::Unreadable)),
    };
    rt.follow.busy = busy;
    job
}

/// The pane is the one a move to another account restarted: the account,
/// when known, is remembered against a launch core had to put elsewhere,
/// and its conversation is resumed.
pub fn moved_in(rt: &mut PaneRt, account: Option<String>) {
    if account.is_some() {
        rt.moved_to = account;
    }
    rt.moved = true;
}

/// What a pane moved to another account says as it starts there: the
/// account core started it on, and whether its conversation came along.
fn moved_note(m: &Model, account: Option<&str>, resumed: bool) -> String {
    let accounts = m.live.accounts.accounts.as_deref().unwrap_or(&[]);
    let id = account.unwrap_or(crate::accounts::MAIN_ACCOUNT);
    let name = crate::accounts::account_label(accounts, id);
    let key = if resumed {
        Key::AccountsMovedNote
    } else {
        Key::AccountsMovedNoteFresh
    };
    Translator::new(m.locale())
        .t_with(key, &args!("name" => name.as_str()))
        .into_owned()
}

/// The session the restart for a role was to resume exists, or not.
pub fn role_restart_answered(
    rt: &PaneRt,
    ws: WsId,
    session: String,
    found: bool,
    locale: Locale,
) -> Task<Message> {
    if found {
        return Task::done(Message::Intent(Intent::ReplacePane {
            ws,
            pane: rt.link.id.clone(),
            patch: Patch::none(),
        }));
    }
    take_restart_note(Some(&session));
    let line = format!(
        "\r\n\x1b[33m{}\x1b[0m\r\n",
        restart_text(locale, RestartText::Failed)
    );
    rt.link.lock().feed(line.as_bytes());
    Task::none()
}

/// A mounted pane without its shell, for what the launch does once the spawn
/// answers, and for the views drawn headless.
#[cfg(test)]
pub(crate) fn mounted(ws: &Workspace, pane: &Pane) -> PaneRt {
    let link = PaneLink::new(
        pane.id.clone(),
        Terminal::new(hangar_term::Config::default(), 80, 24),
        backend_for(ws, pane, &Arc::new(hangar_core::pty::PtyManager::default())),
    );
    PaneRt {
        ws: ws.id.clone(),
        name: pane.name.clone(),
        agent: pane.agent,
        hosted: ws.is_hosted(),
        link,
        status: PaneStatus::Starting,
        notes: Vec::new(),
        dismissed: HashSet::new(),
        search: None,
        last_query: String::new(),
        account: None,
        mcp: None,
        launched_role: pane.role,
        role_handled: false,
        moved_to: None,
        moved: false,
        drafted_at: None,
        follow: FollowRt::default(),
        pending: None,
        start_due: false,
        resume: None,
        owed: false,
        mcp_arg: None,
        look: look::look(&crate::state::Settings::default(), ws, pane.agent),
        focused: false,
        launch: None,
        attach: None,
        seen: 0,
        typed: None,
        parked: None,
    }
}

#[cfg(test)]
mod tests {
    use hangar_core::kits::{GitSource, IssueReason, Source};

    use super::*;
    use crate::panes_logic::agents::launch_command;
    use crate::state::types::{AppState, OrchestrationSettings, WorkspaceKit};

    /// What a workspace that accepted a kit's hooks holds for it.
    const HOOKS: &str = "hooks:0123456789abcdef";

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    /// The verdict, the step and what the header says of a look at a pane
    /// that is to move to `perso`.
    fn look_at(
        rt: &mut PaneRt,
        m: &Model,
        at: Instant,
        job: Job,
    ) -> (Follow, FollowStep, Option<Hold>) {
        let look = account_follow(rt, m, at, job);
        assert_eq!(look.to, "perso");
        (look.verdict, look.step, look.hold)
    }

    /// A Claude Code agent moves by itself to the account its workspace now
    /// uses, on its conversation, and only once Hangar knows it has nothing
    /// to lose: its turn over as its mod reports it, nothing typed in its
    /// prompt, no command of its own running. Whatever is not known waits,
    /// says why in the header, and is looked at again on a timer, never on
    /// every update.
    #[test]
    fn an_agent_moves_only_once_hangar_knows_it_has_nothing_to_lose() {
        use crate::state::live::AgentActivity;
        use crate::workspace_dialog::testing::Harness;

        let mut h = Harness::new(Locale::En, "graphite")
            .edit(|state| state.workspaces[0].claude_account = Some("perso".into()));
        let ws = h.model.workspace("w1").expect("w1").clone();
        let mut rt = mounted(&ws, &ws.panes[0]);
        rt.status = PaneStatus::Running;
        rt.account = Some("main".into());
        let now = Instant::now();

        // No mod has reported: its turn is a guess off the screen.
        assert_eq!(
            look_at(&mut rt, &h.model, now, Job::Unasked),
            (
                Follow::AfterTurn,
                FollowStep::Nothing,
                Some(Hold::Unreported)
            )
        );
        assert!(rt.follow.pending, "its header says it is to move");
        // In a turn, or waiting on the person: after it.
        for busy in [AgentActivity::Working, AgentActivity::Waiting] {
            h.model.live.panes_logic.seed_activity("p1", "w1", busy);
            assert_eq!(
                look_at(&mut rt, &h.model, now, Job::Unasked),
                (Follow::AfterTurn, FollowStep::Nothing, Some(Hold::Turn))
            );
        }
        // The turn is over, but the person typed a prompt not sent yet: a
        // look is set for when it reads as given up, once.
        h.model
            .live
            .panes_logic
            .seed_activity("p1", "w1", AgentActivity::Yours);
        rt.drafted_at = Some(now);
        let expiry = now + DRAFT_HOLD;
        assert_eq!(
            look_at(&mut rt, &h.model, now, Job::Unasked),
            (
                Follow::AfterDraft,
                FollowStep::Wake(expiry),
                Some(Hold::Draft)
            )
        );
        assert_eq!(
            look_at(&mut rt, &h.model, now, Job::Unasked).1,
            FollowStep::Nothing
        );
        // The draft given up: whether a command of its own runs is asked,
        // once.
        woke(&mut rt, expiry);
        assert_eq!(
            look_at(&mut rt, &h.model, expiry, Job::Unasked),
            (Follow::Check, FollowStep::Ask, Some(Hold::Checking))
        );
        assert_eq!(
            look_at(&mut rt, &h.model, expiry, Job::Unasked).1,
            FollowStep::Nothing
        );
        // A build runs below it: the header names it, and it is asked again
        // later.
        let mark = turn_mark(&h.model, "p1");
        let build = Hold::Command("cargo build --release".into());
        let job = job_answered(
            &mut rt,
            &h.model,
            mark,
            PaneJob::Running("cargo build --release".into()),
        );
        assert_eq!(job, Job::Busy);
        let again = expiry + JOB_RECHECK;
        assert_eq!(
            look_at(&mut rt, &h.model, expiry, job),
            (
                Follow::AfterJob,
                FollowStep::Wake(again),
                Some(build.clone())
            )
        );
        // Another update meanwhile asks nothing, and still names it.
        assert_eq!(
            look_at(&mut rt, &h.model, expiry, Job::Unasked),
            (Follow::Check, FollowStep::Nothing, Some(build))
        );
        woke(&mut rt, again);
        assert_eq!(
            look_at(&mut rt, &h.model, again, Job::Unasked).1,
            FollowStep::Ask
        );
        // A process table that cannot be read is as good as a command.
        let job = job_answered(&mut rt, &h.model, mark, PaneJob::Unknown);
        assert_eq!(job, Job::Busy);
        assert_eq!(
            look_at(&mut rt, &h.model, again, job).2,
            Some(Hold::Unreadable)
        );
        let last = again + JOB_RECHECK;
        woke(&mut rt, last);
        assert_eq!(
            look_at(&mut rt, &h.model, last, Job::Unasked).1,
            FollowStep::Ask
        );
        // Nothing runs any more: it moves.
        let job = job_answered(&mut rt, &h.model, mark, PaneJob::Nothing);
        assert_eq!(
            look_at(&mut rt, &h.model, last, job),
            (Follow::Now, FollowStep::Restart, None)
        );
        assert_eq!(rt.moved_to.as_deref(), Some("perso"));
        assert!(!rt.follow.pending);
        // Started for perso: not again, even if core had to fall back.
        assert_eq!(
            look_at(&mut rt, &h.model, last, Job::Nothing),
            (Follow::Stay, FollowStep::Nothing, None)
        );

        // An account not logged in: the pane stays, and says why.
        let mut h = h.edit(|state| state.workspaces[0].claude_account = Some("pro".into()));
        h.model
            .live
            .panes_logic
            .seed_activity("p1", "w1", AgentActivity::Yours);
        let mut rt = mounted(&ws, &ws.panes[0]);
        rt.status = PaneStatus::Running;
        rt.account = Some("main".into());
        let look = account_follow(&mut rt, &h.model, now, Job::Nothing);
        assert_eq!(
            (look.verdict, look.to.as_str(), look.step, look.hold),
            (
                Follow::NotLoggedIn,
                "pro",
                FollowStep::Nothing,
                Some(Hold::NotLoggedIn)
            )
        );

        // Asked to be asked: nothing moves by itself.
        h = h.edit(|state| {
            state.workspaces[0].claude_account = Some("perso".into());
            state.settings.account_switch_ask = true;
        });
        let look = account_follow(&mut rt, &h.model, now, Job::Nothing);
        assert_eq!(
            (look.verdict, look.step),
            (Follow::Stay, FollowStep::Nothing)
        );

        // A shell has no account to follow.
        let h = Harness::new(Locale::En, "graphite")
            .edit(|state| state.workspaces[0].claude_account = Some("perso".into()));
        let mut shell = mounted(&ws, &ws.panes[1]);
        shell.status = PaneStatus::Running;
        shell.account = Some("main".into());
        let look = account_follow(&mut shell, &h.model, now, Job::Nothing);
        assert_eq!(look.verdict, Follow::Stay);
    }

    /// An answer that nothing runs, read while the agent started a turn (and
    /// maybe ended it: `ps` takes its time), is stale: the pane is asked
    /// again instead of restarted.
    #[test]
    fn an_answer_older_than_the_last_turn_is_asked_again() {
        use crate::state::live::{AgentActivity, PaneEvent};
        use crate::workspace_dialog::testing::Harness;

        let mut h = Harness::new(Locale::En, "graphite")
            .edit(|state| state.workspaces[0].claude_account = Some("perso".into()));
        h.model
            .live
            .panes_logic
            .seed_activity("p1", "w1", AgentActivity::Yours);
        let ws = h.model.workspace("w1").expect("w1").clone();
        let mut rt = mounted(&ws, &ws.panes[0]);
        rt.status = PaneStatus::Running;
        rt.account = Some("main".into());
        let now = Instant::now();
        assert_eq!(
            look_at(&mut rt, &h.model, now, Job::Unasked).1,
            FollowStep::Ask
        );
        let mark = turn_mark(&h.model, "p1");

        // A turn starts, and ends, before the process table answers.
        h.model.live.panes_logic.seed_insight("p1", |insight| {
            insight.events.insert(
                0,
                PaneEvent {
                    at: 42,
                    detail: PaneEventDetail::TurnStart,
                },
            );
        });
        assert_ne!(turn_mark(&h.model, "p1"), mark);
        let job = job_answered(&mut rt, &h.model, mark, PaneJob::Nothing);
        assert_eq!(job, Job::Unasked);
        assert_eq!(
            look_at(&mut rt, &h.model, now, job),
            (Follow::Check, FollowStep::Ask, Some(Hold::Checking))
        );
        // Asked again from there, nothing runs: it moves.
        let job = job_answered(
            &mut rt,
            &h.model,
            turn_mark(&h.model, "p1"),
            PaneJob::Nothing,
        );
        assert_eq!(look_at(&mut rt, &h.model, now, job).1, FollowStep::Restart);
    }

    /// A Claude Code on a node starts on the node's login, and its pane
    /// reports main: whether its workspace lives on the node or it is a
    /// worker sent there, it is never restarted toward an account of this
    /// computer (it would come back on main, over and over).
    #[test]
    fn a_pane_on_a_node_never_moves_to_an_account_of_this_computer() {
        use crate::state::live::AgentActivity;
        use crate::workspace_dialog::testing::Harness;

        let claude = |id: &str, node: Option<&str>| Pane {
            id: id.into(),
            name: id.into(),
            agent: AgentId::Claude,
            node: node.map(str::to_owned),
            ..Pane::default()
        };
        let mut h = Harness::new(Locale::En, "graphite").edit(|state| {
            state.settings.claude_account = Some("perso".into());
            state.workspaces[0]
                .panes
                .push(claude("follow-sent", Some("n1")));
            state.workspaces.push(Workspace {
                id: "follow-on-node".into(),
                name: "far".into(),
                cwd: "/srv/far".into(),
                node: Some("n1".into()),
                panes: vec![claude("follow-far", None)],
                ..Workspace::default()
            });
        });
        let now = Instant::now();
        for (ws, pane) in [("w1", "follow-sent"), ("follow-on-node", "follow-far")] {
            h.model
                .live
                .panes_logic
                .seed_activity(pane, ws, AgentActivity::Yours);
            let (ws, pane) = find(&h.model, ws, pane).expect("the pane");
            let mut rt = mounted(ws, pane);
            assert!(rt.link.node().is_some(), "{} runs on its node", pane.id);
            rt.status = PaneStatus::Running;
            rt.account = Some("main".into());
            assert!(!movable(&rt, pane));
            let look = account_follow(&mut rt, &h.model, now, Job::Nothing);
            assert_eq!(
                (look.verdict, look.step, look.hold),
                (Follow::Stay, FollowStep::Nothing, None)
            );
        }
    }

    /// The launch of a pane a move restarted resumes its conversation even
    /// with `auto_resume` off, and says so; without a conversation to
    /// resume, it says it started a new one.
    #[test]
    fn a_moved_pane_resumes_its_conversation_and_says_whether_it_did() {
        use crate::workspace_dialog::testing::Harness;

        let mut h = Harness::new(Locale::En, "graphite").edit(|state| {
            state.settings.auto_resume = false;
            state.workspaces[0].panes[0].session_id = Some("s-moved".into());
        });
        let ws = h.model.workspace("w1").expect("w1").clone();
        let fresh_report = || SpawnReport {
            account: Some("perso".into()),
            resolved: Some("perso".into()),
            ..report(Vec::new())
        };

        // Not moved: the setting decides, and it is off.
        let mut rt = mounted(&ws, &ws.panes[0]);
        let _ = spawned(
            &mut rt,
            &mut h.model,
            &HubChannel::default(),
            Ok(fresh_report()),
        );
        assert!(rt.resume.is_none());

        // Moved: resumed all the same.
        let mut rt = mounted(&ws, &ws.panes[0]);
        moved_in(&mut rt, Some("perso".into()));
        let _ = spawned(
            &mut rt,
            &mut h.model,
            &HubChannel::default(),
            Ok(fresh_report()),
        );
        assert_eq!(rt.resume.as_deref(), Some("s-moved"));
        assert_eq!(rt.moved_to.as_deref(), Some("perso"));
        let tr = Translator::new(Locale::En);
        let said = |key| tr.t_with(key, &args!("name" => "Perso")).into_owned();
        assert_eq!(
            moved_note(&h.model, rt.account.as_deref(), true),
            said(Key::AccountsMovedNote)
        );
        // Its transcript is not found, or it never had a conversation.
        assert_eq!(
            moved_note(&h.model, rt.account.as_deref(), false),
            said(Key::AccountsMovedNoteFresh)
        );
        assert_ne!(
            said(Key::AccountsMovedNote),
            said(Key::AccountsMovedNoteFresh)
        );
    }

    /// A terminal nothing will measure (the chat view's) starts wide and tall
    /// enough for Claude Code to draw a long question whole; one that will be
    /// measured starts at 80×24 and takes its size at the measure.
    #[test]
    fn a_terminal_nothing_measures_starts_big_enough_for_a_long_question() {
        assert_eq!(start_size(true), (80, 24));
        let (columns, rows) = start_size(false);
        assert!(columns >= 160 && rows >= 50, "{columns}×{rows}");
    }

    /// The messages a mount sends, in order, each with when it came.
    fn mount_messages(measured: bool) -> Vec<(Message, Duration)> {
        use iced::futures::StreamExt as _;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let start = std::time::Instant::now();
            let Some(stream) =
                iced_runtime::task::into_stream(mounted_then_due("p1".into(), measured))
            else {
                return Vec::new();
            };
            stream
                .filter_map(|action| async move {
                    match action {
                        iced_runtime::Action::Output(message) => Some(message),
                        _ => None,
                    }
                })
                .map(|message| (message, start.elapsed()))
                .collect()
                .await
        })
    }

    fn is_mounted(message: &Message) -> bool {
        matches!(message, Message::PanesLogic(Logic::Mounted { pane }) if pane == "p1")
    }

    fn is_due(message: &Message) -> bool {
        matches!(message, Message::Panes(Msg::SpawnDue(pane)) if pane == "p1")
    }

    #[test]
    fn a_terminal_nothing_will_measure_has_its_shell_due_right_after_p4_hears_of_it() {
        let sent = mount_messages(false);
        assert_eq!(sent.len(), 2);
        assert!(is_mounted(&sent[0].0), "P4 hears of the pane first");
        assert!(is_due(&sent[1].0), "then the shell is due");
        assert!(sent[1].1 < MEASURE_WAIT, "without the wait for a measure");
    }

    #[test]
    fn a_terminal_about_to_measure_itself_has_its_shell_due_after_the_wait_at_most() {
        let sent = mount_messages(true);
        assert_eq!(sent.len(), 2);
        assert!(is_mounted(&sent[0].0));
        assert!(is_due(&sent[1].0));
        assert!(sent[1].1 >= MEASURE_WAIT, "the measure is waited for");
    }

    fn entry(id: &str, granted: &[&str]) -> WorkspaceKit {
        WorkspaceKit {
            id: id.into(),
            granted: granted.iter().map(|g| (*g).to_owned()).collect(),
        }
    }

    /// A kit installed from GitHub; with `hooks`, one whose plugin runs
    /// something by itself that a workspace has to accept.
    fn git_kit(id: &str, hooks: Option<&str>) -> KitInfo {
        KitInfo {
            id: id.into(),
            source: Source::Git(GitSource {
                url: format!("https://github.com/someone/{id}"),
                git_ref: None,
                subdir: None,
            }),
            dir: format!("/kits/{id}"),
            installed_version: "1.0.0".into(),
            added_at: 0,
            manifest: None,
            error: None,
            update: None,
            hooks_permission: hooks.map(str::to_owned),
            hooks_consent: hooks.is_some(),
            commands_permission: None,
        }
    }

    fn choice(own: Vec<WorkspaceKit>, everywhere: Vec<WorkspaceKit>) -> KitChoice {
        let settings = Settings {
            kits_everywhere: Some(everywhere),
            ..Settings::default()
        };
        let ws = Workspace {
            kits: Some(own),
            ..Workspace::default()
        };
        KitChoice::of(&settings, &ws)
    }

    use super::mounted;

    fn report(kit_issues: Vec<KitLaunchIssue>) -> SpawnReport {
        SpawnReport {
            kit_issues,
            resolved: None,
            missing: None,
            account: None,
            mcp: None,
            mcp_arg: None,
            notes: Vec::new(),
            remote: None,
        }
    }

    #[test]
    fn a_pane_gets_its_workspace_kits_then_the_ones_everywhere() {
        let kits = block_on(launch_kits(
            Some(&choice(
                vec![entry("alpha", &[]), entry("beta", &[])],
                vec![entry("beta", &[]), entry("gamma", &[])],
            )),
            || async { Ok(vec![git_kit("alpha", None), git_kit("gamma", None)]) },
        ));
        // "beta" is not installed: core says so at spawn, so it goes on.
        assert_eq!(kits.ids, ["alpha", "beta", "gamma"]);
        assert!(kits.held.is_empty());
    }

    #[test]
    fn a_git_kit_with_hooks_not_accepted_stays_out_of_the_pane_and_says_so() {
        // "studio" changed what it runs by itself since this workspace
        // enabled it; "quiet" runs nothing; "trusted" was accepted as it is.
        let kits = block_on(launch_kits(
            Some(&choice(
                vec![
                    entry("studio", &[]),
                    entry("quiet", &[]),
                    entry("trusted", &[HOOKS]),
                ],
                vec![],
            )),
            || async {
                Ok(vec![
                    git_kit("studio", Some(HOOKS)),
                    git_kit("quiet", None),
                    git_kit("trusted", Some(HOOKS)),
                ])
            },
        ));
        assert_eq!(kits.ids, ["quiet", "trusted"], "only these reach the spawn");
        assert_eq!(kits.held, ["studio"]);

        // The pane's note bar says so once its shell runs.
        let pane = Pane {
            id: "p1".into(),
            name: "Ava".into(),
            agent: AgentId::Claude,
            ..Pane::default()
        };
        let ws = Workspace {
            id: "w1".into(),
            name: "hangar".into(),
            cwd: "/work/hangar".into(),
            panes: vec![pane.clone()],
            kits: Some(vec![entry("studio", &[])]),
            ..Workspace::default()
        };
        let mut m = Model::default();
        m.hydrate(AppState {
            workspaces: vec![ws.clone()],
            ..AppState::default()
        });
        let mut rt = mounted(&ws, &pane);
        let _ = spawned(
            &mut rt,
            &mut m,
            &HubChannel::default(),
            Ok(report(kit_issues(&kits.held, &[]))),
        );
        let warning = Translator::new(Locale::En)
            .t_with(Key::KitsManageWarnHooks, &args!("id" => "studio"))
            .into_owned();
        assert!(warning.contains("studio"), "{warning}");
        assert_eq!(rt.notes, [warning]);
    }

    /// A pane mounted before the machine's shells were detected starts the
    /// shell it asked for, not the first one found.
    #[test]
    fn the_shell_is_resolved_when_it_starts_not_when_the_pane_mounts() {
        let pane = Pane {
            id: "p1".into(),
            name: "Ava".into(),
            agent: AgentId::Claude,
            shell_id: Some("bash".into()),
            ..Pane::default()
        };
        let ws = Workspace {
            id: "w1".into(),
            name: "hangar".into(),
            cwd: "/work/hangar".into(),
            panes: vec![pane.clone()],
            ..Workspace::default()
        };
        let mut m = Model::default();
        m.hydrate(AppState {
            workspaces: vec![ws.clone()],
            ..AppState::default()
        });
        // At mount, detection has not answered.
        let mut spawn = Spawn {
            id: pane.id.clone(),
            workspace_id: ws.id.clone(),
            workspace_cwd: ws.cwd.clone(),
            cwd: ws.cwd.clone(),
            shell: shell_of(&m, &ws, &pane).cloned(),
            name: pane.name.clone(),
            bridge: false,
            port_base: None,
            build_env: Vec::new(),
            kits: None,
            agent: pane.agent,
            account: None,
            mcp: Some(McpLaunch {
                mode: McpMode::default(),
                enabled: Vec::new(),
                role: false,
                family: None,
                tr: Translator::new(Locale::En),
            }),
            scoped: false,
            node: None,
            away: false,
            hosted: false,
        };
        assert!(spawn.shell.is_none());

        let shell = |id: &str| ShellInfo {
            id: id.into(),
            label: id.into(),
            program: format!("/bin/{id}"),
            args: Vec::new(),
        };
        m.ui.shells = vec![shell("zsh"), shell("bash")];
        spawn.resolve_shell(&m, &ws.id);
        assert_eq!(spawn.shell.as_ref().map(|s| s.id.as_str()), Some("bash"));
        assert_eq!(
            spawn.mcp.as_ref().and_then(|mcp| mcp.family),
            shell_family(spawn.shell.as_ref())
        );
    }

    /// A Codex pane of the old app's file: a shell now, which says so in its
    /// bar the first time it starts, in English or French, and never again.
    /// Saying it saves the pane as the shell it is.
    #[test]
    fn a_pane_of_a_retired_agent_says_once_that_it_is_a_shell_now() {
        let raw = serde_json::from_str(include_str!(
            "../../../hangar-engine/src/state/fixtures/retired_agents.json"
        ))
        .expect("the fixture is JSON");
        let (state, kept) = crate::state::read_checked(raw).expect("the fixture reads");
        let mut m = Model::default();
        m.hydrate_read(state, kept);
        let ws = m.state().workspaces[0].clone();
        let bob = ws.panes[1].clone();
        assert_eq!(
            (bob.agent, bob.retired_agent),
            (AgentId::Shell, Some("Codex"))
        );

        let revision = m.revision();
        let mut rt = mounted(&ws, &bob);
        let _ = spawned(
            &mut rt,
            &mut m,
            &HubChannel::default(),
            Ok(report(Vec::new())),
        );
        let note = Translator::new(Locale::En)
            .t_with(Key::PaneAgentRetired, &args!("agent" => "Codex"))
            .into_owned();
        assert!(note.contains("Codex") && note.contains("shell"), "{note}");
        assert_eq!(rt.notes, [note]);
        let saved = &m.state().workspaces[0].panes[1];
        assert_eq!(saved.retired_agent, None);
        assert!(m.revision() > revision, "the shell is saved");
        assert_eq!(m.to_json()["workspaces"][0]["panes"][1]["agent"], "shell");

        // Restarted or woken up: nothing more to say.
        let mut again = mounted(&ws, saved);
        let _ = spawned(
            &mut again,
            &mut m,
            &HubChannel::default(),
            Ok(report(Vec::new())),
        );
        assert!(again.notes.is_empty());

        // The French says the same, in French.
        let fr = Translator::new(Locale::Fr)
            .t_with(Key::PaneAgentRetired, &args!("agent" => "Gemini CLI"))
            .into_owned();
        assert!(
            fr.contains("Gemini CLI") && fr.contains("plus pris en charge"),
            "{fr}"
        );
    }

    #[test]
    fn the_kits_on_everywhere_are_held_the_same_way() {
        let kits = block_on(launch_kits(
            Some(&choice(vec![], vec![entry("studio", &[])])),
            || async { Ok(vec![git_kit("studio", Some(HOOKS))]) },
        ));
        assert!(kits.ids.is_empty());
        assert_eq!(kits.held, ["studio"]);
    }

    #[test]
    fn a_login_pane_starts_with_no_kit_and_reads_none() {
        let asked = std::cell::Cell::new(false);
        let kits = block_on(launch_kits(None, || {
            asked.set(true);
            async { Ok(Vec::new()) }
        }));
        assert_eq!(kits, PaneKits::default());
        assert!(!asked.get());
    }

    #[test]
    fn kit_notes_put_the_held_kits_first_and_the_forbidden_ones_together() {
        let issues = kit_issues(
            &["studio".to_owned()],
            &[
                LaunchIssue::new("alpha", IssueReason::Missing, None),
                LaunchIssue::new("beta", IssueReason::Forbidden, None),
                LaunchIssue::new("gamma", IssueReason::Forbidden, None),
            ],
        );
        assert_eq!(issues[0].reason, LaunchReason::Hooks);
        let tr = Translator::new(Locale::Fr);
        assert_eq!(
            kit_issue_notes(&issues, tr),
            [
                tr.t_with(Key::KitsManageWarnHooks, &args!("id" => "studio")),
                tr.t_with(Key::KitsWarnMissing, &args!("id" => "alpha")),
                tr.t_with(Key::KitsWarnForbidden, &args!("ids" => "beta, gamma")),
            ]
        );
    }

    fn role_ws(mode: Option<&str>) -> Workspace {
        Workspace {
            orchestration: Some(OrchestrationSettings {
                permission_mode: mode.map(str::to_owned),
                ..OrchestrationSettings::default()
            }),
            ..Workspace::default()
        }
    }

    fn with_role(role: Option<PaneRole>) -> Pane {
        Pane {
            agent: AgentId::Claude,
            role,
            ..Pane::default()
        }
    }

    #[test]
    fn an_orchestration_pane_gets_its_workspace_mode_and_hangars_tools() {
        let ws = role_ws(Some("acceptEdits"));
        let (mode, tools) = role_flags(&ws, &with_role(Some(PaneRole::Worker)));
        assert_eq!(mode.as_deref(), Some("acceptEdits"));
        assert_eq!(tools, ["mcp__hangar"]);
        let options = LaunchOptions {
            permission_mode: mode.as_deref(),
            allowed_tools: &tools,
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, None, &options).as_deref(),
            Some("claude --permission-mode acceptEdits --allowedTools mcp__hangar")
        );
    }

    #[test]
    fn a_mode_that_is_not_offered_is_the_persons_own_default() {
        // Typed into state.json by hand: plain letters, which the command
        // line would have taken, and not a mode the workspace can choose.
        let ws = role_ws(Some("dontAsk"));
        let (mode, tools) = role_flags(&ws, &with_role(Some(PaneRole::Orchestrator)));
        assert_eq!(mode, None);
        assert_eq!(tools, ["mcp__hangar"]);
    }

    #[test]
    fn a_pane_without_a_role_gets_neither() {
        let ws = role_ws(Some("bypassPermissions"));
        assert_eq!(role_flags(&ws, &with_role(None)), (None, Vec::new()));
    }

    // The four cases of mcpScope.test.ts, "launchCommand with a server
    // choice": the argument the launch hands the command, quoted for
    // PowerShell as `mcp_config_arg` quotes it.
    fn ps_file() -> String {
        crate::mcp::scope::quote_arg("C:\\data\\mcp\\ws-1.json", Some(ShellFamily::Powershell))
            .unwrap()
    }

    #[test]
    fn a_server_choice_adds_nothing_in_inherit_mode() {
        assert_eq!(
            launch_command(AgentId::Claude, None, &LaunchOptions::default()).as_deref(),
            Some("claude")
        );
    }

    #[test]
    fn a_server_choice_goes_between_the_resume_and_the_directories() {
        let file = ps_file();
        let roots = vec!["D:\\shared".to_string()];
        let options = LaunchOptions {
            name: Some("Ava"),
            mcp_config: Some(&file),
            extra_roots: &roots,
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, Some("abc"), &options),
            Some(format!(
                "claude --resume abc --name Ava --mcp-config {file} --strict-mcp-config --add-dir \"D:\\shared\""
            ))
        );
    }

    #[test]
    fn a_server_choice_keeps_a_custom_launcher_in_front() {
        let file = ps_file();
        let options = LaunchOptions {
            command: Some("claude --model haiku"),
            mcp_config: Some(&file),
            ..LaunchOptions::default()
        };
        assert_eq!(
            launch_command(AgentId::Claude, None, &options),
            Some(format!(
                "claude --model haiku --mcp-config {file} --strict-mcp-config"
            ))
        );
    }

    #[test]
    fn a_server_choice_leaves_a_shell_alone() {
        let file = ps_file();
        let options = LaunchOptions {
            mcp_config: Some(&file),
            ..LaunchOptions::default()
        };
        assert_eq!(launch_command(AgentId::Shell, Some("s1"), &options), None);
    }

    /// Sixteen panes printing cost one wake-up, then one look a frame: the
    /// window is not built again for every chunk between two frames.
    #[test]
    fn output_wakes_the_window_once_then_is_looked_at_each_frame_until_quiet() {
        use crate::panes::pty_link::HubKind;
        use hangar_core::pty::{PtyEvent, PtyOutput, PtySink};
        let ws = Workspace {
            id: "w".into(),
            ..Workspace::default()
        };
        let pane = |id: &str| Pane {
            id: id.into(),
            ..Pane::default()
        };
        let mut st = crate::panes::State::default();
        let mut wakes = st.channel.take_receiver().expect("the receiver");
        for id in ["a", "b"] {
            st.panes.insert(id.into(), mounted(&ws, &pane(id)));
        }
        let mut sink_a = st.panes["a"].link.sink(st.channel.waker());
        let mut sink_b = st.panes["b"].link.sink(st.channel.waker());
        let print = |sink: &mut PtySink| {
            sink(PtyEvent::Output(PtyOutput {
                id: String::new(),
                data: "x".into(),
                seq: 0,
            }))
        };
        let mut wakes_sent = || {
            let mut outputs = 0;
            while let Ok(event) = wakes.try_recv() {
                if event.kind == HubKind::Output {
                    outputs += 1;
                }
            }
            outputs
        };
        let sorted = |panes: Vec<(PaneId, crate::panes::pty_link::Printed)>| {
            let mut panes: Vec<PaneId> = panes.into_iter().map(|(pane, _)| pane).collect();
            panes.sort();
            panes
        };

        print(&mut sink_a);
        print(&mut sink_a);
        print(&mut sink_b);
        assert_eq!(wakes_sent(), 1, "one wake-up for both panes, three chunks");
        // The wake-up starts the ticking; the panes are looked at.
        st.ticking = true;
        assert_eq!(sorted(crate::panes::take_outputs(&st, None)), ["a", "b"]);

        // While it ticks, output sends nothing: each frame looks.
        print(&mut sink_b);
        assert_eq!(wakes_sent(), 0);
        assert_eq!(sorted(crate::panes::frame(&mut st)), ["b"]);
        assert!(st.ticking);
        // A frame that finds every pane quiet ends it.
        assert!(crate::panes::frame(&mut st).is_empty());
        assert!(!st.ticking);
        // And the next chunk wakes the window again.
        print(&mut sink_a);
        assert_eq!(wakes_sent(), 1);
    }
}
