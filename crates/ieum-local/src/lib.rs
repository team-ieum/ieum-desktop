pub mod ai;
mod files;
mod journal;
mod paths;
mod undo;
mod watch;

pub use files::{FileMoveRunner, FileRenameRunner, FileTrashRunner};
pub use journal::{FileId, FileOp, FileOpKind, Journal, RecentWrites};
pub use paths::{
    AllowedRoots, PathError, resolve_in_base, roots_for_workflow, sanitize_name, user_folders,
    validate_user_folder,
};
pub use undo::{UndoReport, UndoSkip, undo};
pub use watch::{FolderWatcher, NewFile, WatchError, WatchTiming};
