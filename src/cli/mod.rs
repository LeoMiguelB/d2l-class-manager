pub mod handlers;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "d2l",
    version,
    about = "Data getter, document downloader, and AI verification layer for D2L Brightspace / CourseLink",
    long_about = None
)]
pub struct Cli {
    #[arg(long, global = true, help = "Output clean machine-readable JSON")]
    pub json: bool,

    #[arg(long, global = true, help = "Override D2L hostname (default: courselink.uoguelph.ca)")]
    pub host: Option<String>,

    #[arg(long, global = true, help = "Custom path to Obsidian vault (defaults to SCHOOL_DASHBOARD_VAULT)")]
    pub vault: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Log in via browser profile to acquire / refresh Bearer JWT")]
    Login {
        #[arg(long, help = "Force headless browser run")]
        headless: bool,
        #[arg(long, help = "Force refresh even if current token is valid")]
        force: bool,
        #[arg(long, help = "Manually provide token string directly")]
        token: Option<String>,
    },

    #[command(about = "Check current authentication status and user identity")]
    Status,

    #[command(about = "List enrolled courses and mapped OrgUnit IDs")]
    Courses {
        #[arg(long, help = "Only show currently active courses")]
        active: bool,
    },

    #[command(about = "Fetch course announcements (news)")]
    Announcements {
        #[arg(short, long, help = "Filter by course code or OrgUnit ID")]
        course: Option<String>,
        #[arg(long, help = "Only show items since ISO datetime")]
        since: Option<String>,
    },

    #[command(about = "List assignment dropbox folders, due dates, and submissions")]
    Assignments {
        #[arg(short, long, help = "Filter by course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "Fetch course content modules, lecture readings, and syllabus materials")]
    Content {
        #[arg(short, long, help = "Course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "Download lecture slides, course outlines, or assignment attachments")]
    Download {
        #[arg(short, long, required = true, help = "Course code or OrgUnit ID")]
        course: String,
        #[arg(long, default_value = "materials", help = "Type to download: materials | assignments | syllabus")]
        kind: String,
        #[arg(long, help = "Destination folder (defaults to vault course folder)")]
        dest: Option<PathBuf>,
    },

    #[command(about = "Read discussion forum topics, threads, and assigned readings")]
    Posts {
        #[arg(short, long, required = true, help = "Course code or OrgUnit ID")]
        course: String,
        #[arg(long, help = "Specific forum ID")]
        forum: Option<i64>,
        #[arg(long, help = "Specific topic ID")]
        topic: Option<i64>,
    },

    #[command(about = "Fetch calendar events across courses")]
    Calendar {
        #[arg(long, default_value = "14", help = "Number of days ahead to look")]
        days: u32,
    },

    #[command(about = "List quizzes and availability windows")]
    Quizzes {
        #[arg(short, long, help = "Course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "List gradebook items and current marks")]
    Grades {
        #[arg(short, long, help = "Course code or OrgUnit ID")]
        course: Option<String>,
    },

    #[command(about = "Export unified comprehensive JSON snapshot of course state")]
    Dump {
        #[arg(short, long, help = "Specific course code or OrgUnit ID (omit for all)")]
        course: Option<String>,
    },

    #[command(about = "Compare D2L live state with vault deadlines.json and flag discrepancies")]
    Reconcile {
        #[arg(short, long, help = "Specific course code (omit for all vault courses)")]
        course: Option<String>,
        #[arg(long, help = "Apply updates to deadlines.json with .bak backup")]
        apply: bool,
    },
}
