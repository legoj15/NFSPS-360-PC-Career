//! Error type shared by the whole crate.

/// Result alias used throughout [`crate`].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything that can go wrong while scanning FATX / Xbox 360 media.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The source is not recognizable as Xbox 360 media.
    #[error("not an Xbox 360 image: {0}")]
    NotXboxImage(String),

    /// The FATX superblock at `offset` is invalid.
    #[error("invalid FATX superblock at {offset:#x}: {reason}")]
    BadSuperblock {
        /// Byte offset of the partition start within the source.
        offset: u64,
        /// Human-readable reason.
        reason: String,
    },

    /// A directory entry could not be interpreted.
    #[error("invalid directory entry: {0}")]
    BadEntry(String),

    /// A path component was not found while walking the volume.
    #[error("path not found: {0}")]
    NotFound(String),

    /// A path resolved to something that is not a file.
    #[error("not a file: {0}")]
    NotAFile(String),

    /// The FAT chain for a cluster is broken (loop, free entry, out of range).
    #[error("corrupt cluster chain at {cluster}: {reason}")]
    CorruptChain {
        /// Cluster whose chain is broken.
        cluster: u32,
        /// Human-readable reason.
        reason: String,
    },

    /// A file's declared size is not covered by its cluster chain.
    #[error("file truncated: wanted {wanted} bytes, chain provides {got}")]
    Truncated {
        /// Declared file size.
        wanted: u64,
        /// Bytes the chain actually provides.
        got: u64,
    },

    /// The source is not an STFS `CON ` package.
    #[error("not a CON package (magic {0:#?})")]
    NotCon([u8; 4]),

    /// A read returned fewer bytes than the layout requires.
    #[error("short read at offset {offset:#x}: wanted {wanted}, got {got}")]
    ShortRead {
        /// Absolute byte offset of the failed read.
        offset: u64,
        /// Number of bytes requested.
        wanted: usize,
        /// Number of bytes received.
        got: usize,
    },

    /// Underlying I/O failure.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
