use crate::transfer::types::FileEntry;

/// Parse `ls -la` output into FileEntry items.
/// Expects lines like: `drwxr-xr-x 2 user group 4096 2024-01-15 10:30 Documents`
/// or standard ls: `drwxr-xr-x 2 user group 4096 Jan 15 10:30 Documents`
/// Also used by FTP backends to parse LIST responses.
pub fn parse_ls_output(output: &str) -> Vec<FileEntry> {
    let mut entries = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();

        // Skip "total" line and empty lines
        if trimmed.is_empty() || trimmed.starts_with("total ") {
            continue;
        }

        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }

        let permissions = parts[0].to_string();
        let is_dir = permissions.starts_with('d');

        // Try to detect if using --time-style=long-iso (date field is YYYY-MM-DD)
        // long-iso: permissions links user group size YYYY-MM-DD HH:MM name...  (8+ parts)
        // standard: permissions links user group size Mon DD HH:MM name...       (9+ parts)
        let (size, modified, name_start) =
            if parts.len() >= 8 && parts[5].contains('-') && parts[5].len() == 10 {
                let size = parts[4].parse::<u64>().unwrap_or(0);
                let modified = format!("{} {}", parts[5], parts[6]);
                (size, modified, 7)
            } else if parts.len() >= 9 {
                let size = parts[4].parse::<u64>().unwrap_or(0);
                let modified = format!("{} {} {}", parts[5], parts[6], parts[7]);
                (size, modified, 8)
            } else {
                continue;
            };

        // Name is everything from name_start onward (handles spaces in filenames)
        let name = parts[name_start..].join(" ");

        // Skip . entry but keep ..
        if name == "." {
            continue;
        }

        // Handle symlinks: "name -> target" — keep just the name
        let name = if let Some(arrow_pos) = name.find(" -> ") {
            name[..arrow_pos].to_string()
        } else {
            name
        };

        entries.push(FileEntry {
            name,
            is_dir,
            size,
            modified,
            permissions,
        });
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ls_long_iso() {
        let output = "total 48\ndrwxr-xr-x  5 user group  4096 2024-01-15 10:30 Documents\n-rw-r--r--  1 user group 12345 2024-01-14 09:15 file.txt\nlrwxrwxrwx  1 user group    11 2024-01-13 08:00 link -> target\ndrwxr-xr-x  2 user group  4096 2024-01-12 07:00 .\ndrwxr-xr-x  3 user group  4096 2024-01-11 06:00 ..\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0].name, "Documents");
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].size, 4096);

        assert_eq!(entries[1].name, "file.txt");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].size, 12345);

        assert_eq!(entries[2].name, "link");

        assert_eq!(entries[3].name, "..");
        assert!(entries[3].is_dir);
    }

    #[test]
    fn parse_ls_standard_format() {
        let output = "total 8\ndrwxr-xr-x 2 user group 4096 Jan 15 10:30 backups\n-rw-r--r-- 1 user group  420 Jan 14 09:15 config.yml\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name, "backups");
        assert_eq!(entries[1].name, "config.yml");
    }

    #[test]
    fn parse_ls_empty() {
        let entries = parse_ls_output("total 0\n");
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_ls_filename_with_spaces() {
        let output = "total 4\n-rw-r--r-- 1 user group 100 2024-01-15 10:30 my file name.txt\n";
        let entries = parse_ls_output(output);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "my file name.txt");
    }
}
