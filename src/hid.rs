use usbd_human_interface_device::page::Consumer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    VolumeUp,
    VolumeDown,
    Mute,
    PlayPause,
    ScanNext,
    ScanPrevious,
}

/// Maps a logical Command to the matching USB Consumer usage ID.
pub fn command_to_consumer(cmd: Command) -> Consumer {
    match cmd {
        Command::VolumeUp => Consumer::VolumeIncrement,
        Command::VolumeDown => Consumer::VolumeDecrement,
        Command::Mute => Consumer::Mute,
        Command::PlayPause => Consumer::PlayPause,
        Command::ScanNext => Consumer::ScanNextTrack,
        Command::ScanPrevious => Consumer::ScanPreviousTrack,
    }
}
