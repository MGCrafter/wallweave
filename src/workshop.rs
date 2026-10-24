//! Steam owns authentication, subscriptions and content transfer. A separate
//! process owns the SDK context; the GUI never changes its process environment.
use crate::config::Paths;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    thread,
    time::{Duration, Instant},
};
use steamworks::{
    AppIDs, AppId, Client, FileType, ItemState, PublishedFileId, UGCQueryType, UGCType,
};

pub const APP_ID: u32 = 431960;
const PREFIX: &str = "wallweave-workshop:";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const KINDS: &[&str] = &["", "Scene", "Video", "Web"];
pub const RESOLUTIONS: &[&str] = &[
    "",
    "1920 x 1080",
    "2560 x 1440",
    "3840 x 2160",
    "3440 x 1440",
    "2560 x 1080",
    "1080 x 1920",
];
pub const CATEGORIES: &[&str] = &[
    "",
    "Nature",
    "Landscape",
    "Abstract",
    "Anime",
    "Fantasy",
    "Sci-Fi",
    "Technology",
    "Game",
    "Music",
    "Relaxing",
    "Space",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Order {
    #[default]
    #[value(alias = "popular-week")]
    Popular,
    PopularToday,
    PopularMonth,
    PopularYear,
    PopularAllTime,
    Newest,
    Subscriptions,
}

impl Order {
    pub const OPTIONS: &'static [(Self, &'static str)] = &[
        (Self::PopularToday, "Popular Today"),
        (Self::Popular, "Popular Week"),
        (Self::PopularMonth, "Popular Month"),
        (Self::PopularYear, "Popular Year"),
        (Self::PopularAllTime, "Popular All Time"),
        (Self::Newest, "Newest"),
        (Self::Subscriptions, "My subscriptions"),
    ];
    pub fn label(self) -> &'static str {
        Self::OPTIONS
            .iter()
            .find(|(order, _)| *order == self)
            .map(|(_, label)| *label)
            .unwrap_or("Popular Week")
    }
    fn ranking(self) -> (UGCQueryType, Option<u32>) {
        match self {
            Self::PopularToday => (UGCQueryType::RankedByTrend, Some(1)),
            Self::Popular => (UGCQueryType::RankedByTrend, Some(7)),
            Self::PopularMonth => (UGCQueryType::RankedByTrend, Some(30)),
            Self::PopularYear => (UGCQueryType::RankedByTrend, Some(365)),
            Self::PopularAllTime => (UGCQueryType::RankedByVote, None),
            _ => (UGCQueryType::RankedByPublicationDate, None),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Search {
    pub text: String,
    pub page: u32,
    pub order: Order,
    pub kind: String,
    pub resolution: String,
    pub category: String,
    pub show_nsfw: bool,
}

impl Default for Search {
    fn default() -> Self {
        Self {
            text: String::new(),
            page: 1,
            order: Order::Popular,
            kind: String::new(),
            resolution: String::new(),
            category: String::new(),
            show_nsfw: false,
        }
    }
}

impl Search {
    fn validate(&self) -> Result<()> {
        ensure!(
            self.page > 0 && self.page <= 100_000,
            "Invalid Workshop page"
        );
        ensure!(
            self.text.len() <= 512 && !self.text.contains('\0'),
            "Search is too long or contains an invalid character"
        );
        ensure!(
            KINDS.contains(&self.kind.as_str()),
            "Unknown wallpaper type"
        );
        ensure!(
            RESOLUTIONS.contains(&self.resolution.as_str()),
            "Unknown resolution"
        );
        ensure!(
            CATEGORIES.contains(&self.category.as_str()),
            "Unknown category"
        );
        Ok(())
    }
}

/// Accept a numeric ID or an actual Steam Workshop item link, never a shell
/// command, arbitrary URL, collection contents, or an ID embedded in prose.
pub fn item_id(input: &str) -> Option<u64> {
    let input = input.trim();
    let value = if input.bytes().all(|c| c.is_ascii_digit()) {
        input
    } else {
        let path = input
            .strip_prefix("https://steamcommunity.com/sharedfiles/filedetails/?")
            .or_else(|| input.strip_prefix("https://steamcommunity.com/workshop/filedetails/?"))?;
        path.split('&').find_map(|part| part.strip_prefix("id="))?
    };
    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    value.parse::<u64>().ok().filter(|id| *id > 0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub preview_url: Option<String>,
    pub bytes: u64,
    pub votes: u32,
    pub installed: bool,
    pub subscribed: bool,
    /// Steam's list-added time, only meaningful for a subscription-list query.
    #[serde(default)]
    pub subscribed_at: Option<u32>,
    #[serde(default)]
    pub nsfw: bool,
}

impl Item {
    pub fn kind(&self) -> &str {
        self.tags
            .iter()
            .find(|tag| ["Scene", "Video", "Web", "Application"].contains(&tag.as_str()))
            .map(String::as_str)
            .unwrap_or("Wallpaper")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<Item>,
    pub total: u32,
    pub page: u32,
    pub has_next: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Phase {
    Waiting,
    Checking,
    Subscribing,
    Queued,
    Downloading,
    Installing,
    Complete,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Download {
    pub id: u64,
    pub phase: Phase,
    pub downloaded: u64,
    pub total: u64,
    pub folder: Option<PathBuf>,
    pub error: Option<String>,
}

impl Download {
    pub fn active(&self) -> bool {
        !matches!(self.phase, Phase::Complete | Phase::Failed)
    }
    pub fn fraction(&self) -> Option<f32> {
        (self.total > 0)
            .then(|| (self.downloaded as f64 / self.total as f64).clamp(0.0, 1.0) as f32)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum Command {
    Search { request: u64, search: Search },
    Download { id: u64 },
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Connected,
    Results {
        request: u64,
        page: Page,
    },
    Download {
        download: Download,
    },
    ItemTitle {
        id: u64,
        title: String,
    },
    Error {
        request: Option<u64>,
        message: String,
    },
    Disconnected {
        message: String,
    },
}

/// The controller owns exactly one child, and terminates/reaps that child on
/// disconnect or drop. No GUI wait, global Steam shutdown, or account storage.
pub struct Session {
    sender: Sender<Command>,
    receiver: Receiver<Event>,
}

impl Session {
    pub fn spawn(paths: &Paths) -> Self {
        Self::spawn_executable(paths, std::env::current_exe())
    }

    pub(crate) fn spawn_executable(paths: &Paths, executable: std::io::Result<PathBuf>) -> Self {
        let (sender, commands) = mpsc::channel();
        let (events, receiver) = mpsc::channel();
        let state = paths.state.clone();
        thread::spawn(move || {
            let result = (|| -> Result<()> {
                fs::create_dir_all(&state)?;
                let log = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(state.join("workshop.log"))?;
                let mut child = ProcessCommand::new(executable?)
                    .arg("workshop-worker")
                    .env("SteamAppId", APP_ID.to_string())
                    .env("SteamGameId", APP_ID.to_string())
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(log)
                    .spawn()
                    .context("Start Steam Workshop connection")?;
                let result = (|| -> Result<()> {
                    let mut input = child.stdin.take().context("Workshop input unavailable")?;
                    let output = child.stdout.take().context("Workshop output unavailable")?;
                    let ready = Arc::new(AtomicBool::new(false));
                    let seen = ready.clone();
                    let tx = events.clone();
                    thread::spawn(move || {
                        for line in BufReader::new(output).lines() {
                            let Ok(line) = line else { break };
                            if let Some(json) = line.strip_prefix(PREFIX)
                                && let Ok(event) = serde_json::from_str::<Event>(json)
                            {
                                seen.store(true, Ordering::Relaxed);
                                if tx.send(event).is_err() {
                                    break;
                                }
                            }
                        }
                    });
                    let started = Instant::now();
                    loop {
                        if let Some(status) = child.try_wait()? {
                            bail!(
                                "Steam Workshop connection closed ({status}). Start Steam, sign in, then reconnect."
                            );
                        }
                        ensure!(
                            ready.load(Ordering::Relaxed) || started.elapsed() < REQUEST_TIMEOUT,
                            "Steam did not answer. Restart Steam and reconnect."
                        );
                        match commands.recv_timeout(Duration::from_millis(100)) {
                            Ok(command) => {
                                serde_json::to_writer(&mut input, &command)?;
                                input.write_all(b"\n")?;
                                input.flush()?;
                                if matches!(command, Command::Shutdown) {
                                    break;
                                }
                            }
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                        }
                    }
                    Ok(())
                })();
                // Closing stdin tells the worker to shut down. Bound the wait
                // even when a native SDK call is stuck.
                drop(child.stdin.take());
                let until = Instant::now() + Duration::from_secs(2);
                while child.try_wait()?.is_none() && Instant::now() < until {
                    thread::sleep(Duration::from_millis(25));
                }
                if child.try_wait()?.is_none() {
                    let _ = child.kill();
                }
                let _ = child.wait();
                result
            })();
            if let Err(error) = result {
                let _ = events.send(Event::Disconnected {
                    message: format!("{error:#}"),
                });
            }
        });
        Self { sender, receiver }
    }

    pub fn send(&self, command: Command) -> Result<()> {
        self.sender
            .send(command)
            .context("Steam connection is closed; reconnect in Workshop")
    }
    pub fn try_recv(&self) -> std::result::Result<Event, mpsc::TryRecvError> {
        self.receiver.try_recv()
    }
    pub fn recv_timeout(
        &self,
        timeout: Duration,
    ) -> std::result::Result<Event, mpsc::RecvTimeoutError> {
        self.receiver.recv_timeout(timeout)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sender.send(Command::Shutdown);
    }
}

fn emit(event: Event) -> Result<()> {
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    write!(out, "{PREFIX}")?;
    serde_json::to_writer(&mut out, &event)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}

enum Reply {
    Search(u64, std::result::Result<Page, String>),
    Checked(u64, std::result::Result<String, String>),
    Subscribed(u64, std::result::Result<(), String>),
    Finished(u64, Option<String>),
}

fn valid_item(item: &steamworks::QueryResult) -> bool {
    item.consumer_app_id == Some(AppId(APP_ID))
        && !item.banned
        && item.file_type == FileType::Community
}

fn nsfw_tags(tags: &[String]) -> bool {
    tags.iter()
        .any(|tag| ["mature", "questionable", "nsfw"].contains(&tag.to_ascii_lowercase().as_str()))
}

fn search(client: &Client, request: u64, search: Search, tx: Sender<Reply>) -> Result<()> {
    search.validate()?;
    let ugc = client.ugc();
    let direct = item_id(&search.text);
    let mut query = if let Some(id) = direct {
        ugc.query_item(PublishedFileId(id))?
    } else if search.order == Order::Subscriptions {
        ugc.query_user(
            client.user().steam_id().account_id(),
            steamworks::UserList::Subscribed,
            UGCType::Items,
            steamworks::UserListOrder::SubscriptionDateDesc,
            AppIDs::ConsumerAppId(AppId(APP_ID)),
            search.page,
        )?
    } else {
        let (ranking, days) = search.order.ranking();
        let query = ugc.query_all(
            ranking,
            UGCType::Items,
            AppIDs::ConsumerAppId(AppId(APP_ID)),
            search.page,
        )?;
        if let Some(days) = days {
            query.set_ranked_by_trend_days(days)
        } else {
            query
        }
    };
    if direct.is_none() {
        if !search.text.trim().is_empty() {
            query = query.set_search_text(search.text.trim());
        }
        for tag in [&search.kind, &search.resolution, &search.category] {
            if !tag.is_empty() {
                query = query.require_tag(tag);
            }
        }
        if !search.show_nsfw {
            for tag in ["Mature", "Questionable", "NSFW"] {
                query = query.exclude_tag(tag);
            }
        }
    }
    query.allow_cached_response(30).fetch(move |result| {
        let page = result
            .map(|results| {
                let total = results.total_results();
                let returned = results.returned_results();
                let items = (0..returned)
                    .filter_map(|i| {
                        let item = results.get(i)?;
                        if !valid_item(&item) {
                            return None;
                        }
                        let nsfw = nsfw_tags(&item.tags)
                            || results.content_descriptor(i).iter().any(|descriptor| {
                                matches!(descriptor,
                            steamworks::UGCContentDescriptorID::NudityOrSexualContent |
                            steamworks::UGCContentDescriptorID::AdultOnlySexualContent |
                            steamworks::UGCContentDescriptorID::GratuitousSexualContent |
                            steamworks::UGCContentDescriptorID::AnyMatureContent)
                            });
                        if nsfw && !search.show_nsfw {
                            return None;
                        }
                        Some(Item {
                            id: item.published_file_id.0,
                            title: item.title,
                            description: item.description,
                            tags: item.tags,
                            preview_url: results.preview_url(i),
                            bytes: item.file_size as u64,
                            votes: item.num_upvotes,
                            installed: false,
                            subscribed: false,
                            subscribed_at: (direct.is_none()
                                && search.order == Order::Subscriptions
                                && item.time_added_to_user_list > 0)
                                .then_some(item.time_added_to_user_list),
                            nsfw,
                        })
                    })
                    .collect();
                Page {
                    items,
                    total,
                    page: search.page,
                    has_next: direct.is_none()
                        && search.page.saturating_mul(steamworks::RESULTS_PER_PAGE) < total,
                }
            })
            .map_err(|e| e.to_string());
        let _ = tx.send(Reply::Search(request, page));
    });
    Ok(())
}

struct Job {
    download: Download,
    changed: Instant,
}

fn publish(job: &Job) -> Result<()> {
    emit(Event::Download {
        download: job.download.clone(),
    })
}

fn fail(job: &mut Job, error: String) -> Result<()> {
    job.download.phase = Phase::Failed;
    job.download.error = Some(error);
    publish(job)
}

fn complete(client: &Client, job: &mut Job) -> Result<()> {
    if let Some(info) = client
        .ugc()
        .item_install_info(PublishedFileId(job.download.id))
    {
        job.download.folder = Some(PathBuf::from(info.folder));
        job.download.phase = Phase::Complete;
        job.download.downloaded = info.size_on_disk;
        job.download.total = info.size_on_disk;
        publish(job)
    } else {
        fail(job, "Steam finished the download but did not report an installation folder. Retry or check Steam Downloads.".into())
    }
}

pub fn worker() -> Result<()> {
    if let Err(error) = worker_inner() {
        emit(Event::Disconnected {
            message: format!("{error:#}"),
        })?;
    }
    Ok(())
}

fn worker_inner() -> Result<()> {
    let paths = Paths::discover()?;
    fs::create_dir_all(&paths.state)?;
    let connection_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(paths.state.join("workshop-connection.lock"))?;
    fs2::FileExt::try_lock_exclusive(&connection_lock).context(
        "Another Wallweave window already owns the Steam connection. Use its Workshop tab",
    )?;
    crate::library::preserve_downloads(&paths.load()?)
        .context("Preserve completed Steam downloads before connecting")?;
    let client = Client::init().context("Connect to Steam. Start the native Steam client, sign in with the account that owns Wallpaper Engine, then reconnect")?;
    ensure!(
        client.user().logged_on(),
        "Steam is offline. Sign in and reconnect."
    );
    ensure!(
        client.apps().is_subscribed(),
        "The signed-in Steam account needs Wallpaper Engine."
    );
    emit(Event::Connected)?;
    let (commands_tx, commands) = mpsc::channel();
    thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            let command = if line.len() <= 8192 {
                serde_json::from_str(&line).ok()
            } else {
                None
            };
            if let Some(command) = command {
                if commands_tx.send(command).is_err() {
                    return;
                }
            } else {
                break;
            }
        }
        let _ = commands_tx.send(Command::Shutdown);
    });
    let (tx, replies) = mpsc::channel();
    let finished_tx = tx.clone();
    let _finished = client.register_callback(move |result: steamworks::DownloadItemResult| {
        if result.app_id == AppId(APP_ID) {
            let _ = finished_tx.send(Reply::Finished(
                result.published_file_id.0,
                result.error.map(|e| e.to_string()),
            ));
        }
    });
    let mut searches = HashMap::<u64, Instant>::new();
    let mut jobs = BTreeMap::<u64, Job>::new();
    let mut waiting = VecDeque::new();
    let mut polled = Instant::now();
    let mut online_check = Instant::now();
    loop {
        client.run_callbacks();
        while let Ok(command) = commands.try_recv() {
            match command {
                Command::Shutdown => return Ok(()),
                Command::Search {
                    request,
                    search: terms,
                } => {
                    let result = if searches.len() >= 4 {
                        Err(anyhow::anyhow!("A Workshop search is already pending"))
                    } else {
                        search(&client, request, terms, tx.clone())
                    };
                    match result {
                        Ok(()) => {
                            searches.insert(request, Instant::now());
                        }
                        Err(error) => emit(Event::Error {
                            request: Some(request),
                            message: format!("{error:#}"),
                        })?,
                    }
                }
                Command::Download { id } => {
                    if id == 0 || jobs.get(&id).is_some_and(|j| j.download.active()) {
                        continue;
                    }
                    let job = Job {
                        download: Download {
                            id,
                            phase: Phase::Waiting,
                            downloaded: 0,
                            total: 0,
                            folder: None,
                            error: None,
                        },
                        changed: Instant::now(),
                    };
                    publish(&job)?;
                    jobs.insert(id, job);
                    waiting.push_back(id);
                }
            }
        }
        if !jobs
            .values()
            .any(|job| job.download.active() && job.download.phase != Phase::Waiting)
            && let Some(id) = waiting.pop_front()
        {
            crate::library::preserve_downloads(&paths.load()?)
                .context("Preserve completed Steam downloads before starting another item")?;
            if let Some(job) = jobs.get_mut(&id) {
                job.download.phase = Phase::Checking;
                job.changed = Instant::now();
                match client.ugc().query_item(PublishedFileId(id)) {
                    Ok(query) => {
                        let checked_tx = tx.clone();
                        query.fetch(move |result| {
                            let checked = result.map_err(|e| e.to_string()).and_then(|r| {
                                r.get(0).filter(valid_item).map(|item| item.title)
                                    .ok_or_else(|| "This item is unavailable, a collection, or belongs to another app.".into())
                            });
                            let _ = checked_tx.send(Reply::Checked(id, checked));
                        });
                        publish(job)?;
                    }
                    Err(error) => fail(job, error.to_string())?,
                }
            }
        }
        while let Ok(reply) = replies.try_recv() {
            match reply {
                Reply::Search(request, result) => {
                    if searches.remove(&request).is_none() {
                        continue;
                    }
                    match result {
                        Ok(mut page) => {
                            for item in &mut page.items {
                                let state = client.ugc().item_state(PublishedFileId(item.id));
                                item.installed = state.contains(ItemState::INSTALLED)
                                    && !state.intersects(
                                        ItemState::NEEDS_UPDATE
                                            | ItemState::DOWNLOADING
                                            | ItemState::DOWNLOAD_PENDING,
                                    );
                                item.subscribed = state.contains(ItemState::SUBSCRIBED);
                            }
                            emit(Event::Results { request, page })?;
                        }
                        Err(message) => emit(Event::Error {
                            request: Some(request),
                            message,
                        })?,
                    }
                }
                Reply::Checked(id, result) => {
                    let Some(job) = jobs
                        .get_mut(&id)
                        .filter(|j| j.download.phase == Phase::Checking)
                    else {
                        continue;
                    };
                    match result {
                        Err(error) => {
                            fail(job, error)?;
                            continue;
                        }
                        Ok(title) => emit(Event::ItemTitle { id, title })?,
                    }
                    job.download.phase = Phase::Subscribing;
                    job.changed = Instant::now();
                    publish(job)?;
                    let subscribed_tx = tx.clone();
                    client
                        .ugc()
                        .subscribe_item(PublishedFileId(id), move |result| {
                            let _ = subscribed_tx
                                .send(Reply::Subscribed(id, result.map_err(|e| e.to_string())));
                        });
                }
                Reply::Subscribed(id, result) => {
                    let Some(job) = jobs
                        .get_mut(&id)
                        .filter(|j| j.download.phase == Phase::Subscribing)
                    else {
                        continue;
                    };
                    if let Err(error) = result {
                        fail(job, error)?;
                        continue;
                    }
                    let state = client.ugc().item_state(PublishedFileId(id));
                    if state.contains(ItemState::INSTALLED)
                        && !state.intersects(
                            ItemState::NEEDS_UPDATE
                                | ItemState::DOWNLOADING
                                | ItemState::DOWNLOAD_PENDING,
                        )
                    {
                        complete(&client, job)?;
                    } else if client.ugc().download_item(PublishedFileId(id), false) {
                        job.download.phase = Phase::Queued;
                        publish(job)?;
                    } else {
                        fail(job, "Steam could not start this download. Check the connection, free disk space and Steam Downloads, then retry.".into())?;
                    }
                }
                Reply::Finished(id, error) => {
                    let Some(job) = jobs.get_mut(&id).filter(|j| {
                        matches!(
                            j.download.phase,
                            Phase::Queued | Phase::Downloading | Phase::Installing
                        )
                    }) else {
                        continue;
                    };
                    if let Some(error) = error {
                        fail(job, error)?;
                    } else {
                        complete(&client, job)?;
                    }
                }
            }
        }
        if polled.elapsed() >= Duration::from_millis(500) {
            polled = Instant::now();
            let expired: Vec<_> = searches
                .iter()
                .filter(|(_, start)| start.elapsed() > REQUEST_TIMEOUT)
                .map(|(id, _)| *id)
                .collect();
            for request in expired {
                searches.remove(&request);
                emit(Event::Error {
                    request: Some(request),
                    message: "Steam Workshop search timed out. Try again.".into(),
                })?;
            }
            for job in jobs.values_mut().filter(|j| j.download.active()) {
                if job.download.phase == Phase::Waiting {
                    continue;
                }
                if matches!(job.download.phase, Phase::Checking | Phase::Subscribing) {
                    if job.changed.elapsed() > REQUEST_TIMEOUT {
                        fail(job, "Steam did not answer this request. Retry.".into())?;
                    }
                    continue;
                }
                let before = job.download.clone();
                let id = PublishedFileId(job.download.id);
                if let Some((downloaded, total)) = client.ugc().item_download_info(id) {
                    job.download.downloaded = downloaded;
                    job.download.total = total;
                }
                let state = client.ugc().item_state(id);
                job.download.phase =
                    if job.download.total > 0 && job.download.downloaded >= job.download.total {
                        Phase::Installing
                    } else if state.contains(ItemState::DOWNLOADING) {
                        Phase::Downloading
                    } else {
                        Phase::Queued
                    };
                // Completion is exclusively reported after DownloadItemResult.
                if before != job.download {
                    publish(job)?;
                }
            }
        }
        if online_check.elapsed() >= Duration::from_secs(2) {
            online_check = Instant::now();
            ensure!(
                client.user().logged_on(),
                "Steam disconnected. Sign in, then reconnect. Downloads remain managed by Steam."
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
}

pub fn preview_path(state: &Path, item: &Item) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut hash = std::hash::DefaultHasher::new();
    item.preview_url.as_ref()?.hash(&mut hash);
    Some(
        state
            .join("workshop-previews")
            .join(format!("{}-{:x}.preview", item.id, hash.finish())),
    )
}

/// Fetch only Steam-hosted artwork with bounded size/time and no redirects.
/// Authentication is never included in these requests.
pub fn fetch_preview(url: &str, path: &Path) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    let uri: ureq::http::Uri = url.parse()?;
    let host = uri.host().unwrap_or("");
    ensure!(
        uri.scheme_str() == Some("https")
            && [
                "steamusercontent.com",
                "steamstatic.com",
                "steamcontent.com"
            ]
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
            || uri.scheme_str() == Some("https") && host == "steamuserimages-a.akamaihd.net",
        "Preview is not hosted on a Steam image server"
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .timeout_global(Some(Duration::from_secs(12)))
        .build()
        .into();
    let mut response = agent.get(url).call()?;
    ensure!(
        response.status() == 200,
        "Preview server did not return an image"
    );
    let bytes = response
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_vec()?;
    let parent = path.parent().context("Missing preview cache directory")?;
    fs::create_dir_all(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&bytes)?;
    file.persist(path).map_err(|e| e.error)?;
    // Bound the persistent artwork cache as well as the GUI's texture cache.
    let mut cached: Vec<_> = fs::read_dir(parent)?
        .flatten()
        .filter_map(|e| {
            let metadata = e.metadata().ok()?;
            (metadata.is_file() && e.path().extension().is_some_and(|ext| ext == "preview"))
                .then(|| (metadata.modified().ok(), e.path()))
        })
        .collect();
    cached.sort_by_key(|(modified, _)| *modified);
    let excess = cached.len().saturating_sub(256);
    for (_, old) in cached.into_iter().take(excess) {
        if old != path {
            let _ = fs::remove_file(old);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popular_periods_use_steam_trends_and_all_time_uses_lifetime_votes() {
        for (order, days) in [
            (Order::PopularToday, 1),
            (Order::Popular, 7),
            (Order::PopularMonth, 30),
            (Order::PopularYear, 365),
        ] {
            assert_eq!(order.ranking(), (UGCQueryType::RankedByTrend, Some(days)));
        }
        assert_eq!(
            Order::PopularAllTime.ranking(),
            (UGCQueryType::RankedByVote, None)
        );
        assert_eq!(
            Order::Newest.ranking(),
            (UGCQueryType::RankedByPublicationDate, None)
        );
    }

    #[test]
    fn nsfw_filter_covers_wallpaper_engine_ratings_and_defaults_to_hidden() {
        assert!(!Search::default().show_nsfw);
        for tag in ["Mature", "Questionable", "NSFW", "mature"] {
            assert!(nsfw_tags(&["Scene".into(), tag.into()]));
        }
        assert!(!nsfw_tags(&["Everyone".into(), "Scene".into()]));
        let old: Search = serde_json::from_str(r#"{"text":"forest","page":1,"order":"Popular","kind":"","resolution":"","category":""}"#).expect("older query");
        assert!(!old.show_nsfw);
    }

    #[test]
    fn download_validation_rejects_foreign_banned_and_collection_items() {
        let mut item = steamworks::QueryResult {
            published_file_id: PublishedFileId(123),
            creator_app_id: Some(AppId(APP_ID)),
            consumer_app_id: Some(AppId(APP_ID)),
            title: "Test wallpaper".into(),
            description: String::new(),
            owner: steamworks::SteamId::from_raw(1),
            time_created: 0,
            time_updated: 0,
            time_added_to_user_list: 0,
            visibility: steamworks::PublishedFileVisibility::Public,
            banned: false,
            accepted_for_use: true,
            tags: vec![],
            tags_truncated: false,
            file_name: String::new(),
            file_type: FileType::Community,
            file_size: 1,
            url: String::new(),
            num_upvotes: 0,
            num_downvotes: 0,
            score: 0.0,
            num_children: 0,
        };
        assert!(valid_item(&item));
        item.consumer_app_id = Some(AppId(730));
        assert!(!valid_item(&item));
        item.consumer_app_id = Some(AppId(APP_ID));
        item.file_type = FileType::Collection;
        assert!(!valid_item(&item));
        item.file_type = FileType::Community;
        item.banned = true;
        assert!(!valid_item(&item));
    }
    #[test]
    fn ids_are_strict_and_links_cannot_inject_commands() {
        assert_eq!(item_id("12345"), Some(12345));
        assert_eq!(
            item_id(
                "https://steamcommunity.com/sharedfiles/filedetails/?id=12345&searchtext=forest"
            ),
            Some(12345)
        );
        for invalid in [
            "0",
            "",
            "18446744073709551616",
            "12;shutdown",
            "https://example.com/?id=123",
            "https://steamcommunity.com.evil/sharedfiles/filedetails/?id=123",
            "some wallpaper 123",
        ] {
            assert_eq!(item_id(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn validates_search_before_calling_native_api() {
        let mut search = Search::default();
        assert!(search.validate().is_ok());
        search.text = "forest\0shutdown".into();
        assert!(search.validate().is_err());
        search.text.clear();
        search.page = 0;
        assert!(search.validate().is_err());
        search.page = 1;
        search.kind = "bad\0tag".into();
        assert!(search.validate().is_err());
    }

    #[test]
    fn progress_is_bounded_and_zero_totals_are_indeterminate() {
        let mut d = Download {
            id: 1,
            phase: Phase::Queued,
            downloaded: 10,
            total: 0,
            folder: None,
            error: None,
        };
        assert_eq!(d.fraction(), None);
        assert!(d.active());
        d.total = 5;
        assert_eq!(d.fraction(), Some(1.0));
        d.phase = Phase::Failed;
        assert!(!d.active());
    }

    #[test]
    fn refuses_non_steam_preview_hosts_without_fetching() {
        let dir = tempfile::tempdir().expect("temporary directory");
        for url in [
            "http://images.steamusercontent.com/a",
            "https://localhost/a",
            "https://steamstatic.com.evil/a",
            "file:///etc/passwd",
        ] {
            assert!(fetch_preview(url, &dir.path().join("image")).is_err());
        }
    }

    #[test]
    fn bridge_ignores_sdk_output_delivers_events_and_reaps_its_child() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().expect("temporary directory");
        let script = directory.path().join("fake-steam-worker");
        fs::write(&script, r##"#!/usr/bin/env python3
import json, os, pathlib, sys
pathlib.Path(__file__).with_suffix('.pid').write_text(str(os.getpid()))
assert os.environ['SteamAppId'] == '431960'
assert os.environ['SteamGameId'] == '431960'
print('[S_API] harmless native SDK output', flush=True)
print('wallweave-workshop:' + json.dumps({'event':'connected'}), flush=True)
for line in sys.stdin:
    command = json.loads(line)
    if command['command'] == 'shutdown':
        break
    if command['command'] == 'search':
        print('wallweave-workshop:' + json.dumps({'event':'results', 'request':command['request'], 'page':{'items':[], 'total':0, 'page':command['search']['page'], 'has_next':False}}), flush=True)
"##).expect("worker fixture");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).expect("executable");
        let paths = Paths {
            config: directory.path().join("config.toml"),
            state: directory.path().join("state"),
            runtime: directory.path().join("run"),
        };
        let session = Session::spawn_executable(&paths, Ok(script.clone()));
        assert!(matches!(
            session.recv_timeout(Duration::from_secs(5)),
            Ok(Event::Connected)
        ));
        session
            .send(Command::Search {
                request: 42,
                search: Search::default(),
            })
            .expect("search command");
        assert!(matches!(
            session.recv_timeout(Duration::from_secs(5)),
            Ok(Event::Results { request: 42, .. })
        ));
        let pid = fs::read_to_string(script.with_extension("pid")).expect("child pid");
        drop(session);
        let deadline = Instant::now() + Duration::from_secs(4);
        let proc = PathBuf::from(format!("/proc/{pid}"));
        while proc.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !proc.exists(),
            "dropping the bridge must terminate and reap its worker"
        );
    }

    #[test]
    fn failed_worker_start_is_reported_without_blocking_the_caller() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let paths = Paths {
            config: directory.path().join("config.toml"),
            state: directory.path().join("state"),
            runtime: directory.path().join("run"),
        };
        let session =
            Session::spawn_executable(&paths, Ok(directory.path().join("missing-worker")));
        assert!(matches!(
            session.recv_timeout(Duration::from_secs(5)),
            Ok(Event::Disconnected { .. })
        ));
    }
}
