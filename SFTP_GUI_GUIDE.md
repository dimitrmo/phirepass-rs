# SFTP GUI Implementation Guide

## Overview
This document describes the new SFTP file browser GUI that has been added to the Phirepass web console alongside the existing SSH terminal functionality.

## Features

### Tab-Based Interface
- **SSH Tab**: Original SSH terminal experience (xterm.js)
- **SFTP Tab**: New file browser for remote file operations

The tab navigation is hidden until a node is selected. Once a node is selected, both tabs become available.

### SFTP Browser Capabilities
- **File Listing**: View files and directories in the remote system
- **Directory Navigation**: Click on folders to navigate into them, use "Back" button to go up
- **Path Display**: Real-time path indicator showing current location
- **File Information**: Display file sizes and file type icons
- **Refresh Capability**: Manually refresh the current directory listing

### Authentication Handling
- **Automatic Credential Modal**: When authentication is required (username/password not provided), a modal dialog appears
- **Credential Caching**: Credentials are used to establish the SFTP tunnel
- **Error Handling**: Authentication errors trigger the credential modal for retry

## File Structure

### New/Modified Files

#### 1. **channel/public/index.html**
- Added tab navigation bar with SSH and SFTP buttons
- Added tab content containers for each mode
- Added SFTP container with file browser UI
- Added credentials modal for authentication

#### 2. **channel/public/phirepass.js** (Modified)
- Imported SFTPBrowser module
- Added tab switching functionality
- Updated node selection to show tabs
- Modified protocol message handler to route messages to appropriate handler (SSH or SFTP)
- Added tab event listeners
- Added SFTP tunnel initialization on SFTP tab selection

#### 3. **channel/public/sftp.js** (New)
- `SFTPBrowser` class for managing SFTP session
- Directory listing and navigation
- Credentials modal management
- File/folder UI rendering
- Error handling

#### 4. **channel/public/phirepass.css** (Modified)
- Added tab button styles (.tab-button, .tab-button.active)
- Added tab content styles (.tab-content, .tab-content.active)
- Added SFTP item styles (.sftp-item, .sftp-item-icon, .sftp-item-name, etc.)
- Added modal styling for credentials input

#### 5. **channel/Cargo.toml** (Modified)
- Added "stats" and "node" features to phirepass-common dependency for WASM compilation

## How It Works

### User Flow

1. **Node Selection**
   - User views list of available nodes
   - Selects a node by clicking on its card
   - Tab navigation appears automatically

2. **SSH Connection** (Default)
   - SSH tab is active by default
   - WebSocket connection established
   - SSH tunnel opened
   - Terminal becomes interactive

3. **SFTP Connection**
   - User clicks on SFTP tab
   - If no credentials required: root directory listing loads automatically
   - If credentials required: modal appears for username/password input
   - After credentials: file browser opens at root directory

4. **Browsing Files**
   - User sees file/folder list with sizes
   - Clicking folder navigates into it
   - "Back" button goes to parent directory
   - "Refresh" button reloads current directory

5. **Error Handling**
   - Authentication errors show credential modal
   - Network errors display error message in browser
   - Users can retry with new credentials

### Protocol Communication

The implementation uses existing WebFrameData messages:

- **OpenTunnel** (protocol=1 for SFTP)
  - Initiates SFTP tunnel with optional username/password
  - Sent when SFTP tab is clicked

- **TunnelOpened**
  - Indicates tunnel is ready
  - Triggers initial directory listing

- **SFTPList**
  - Requests file listing for a path
  - Sent with path, session ID, and message ID

- **SFTPListItems**
  - Individual file/directory items from server
  - Contains file metadata (name, size, is_dir)
  - Multiple messages sent for complete listing

- **TunnelClosed**
  - Indicates tunnel closure
  - SFTP browser disconnects

- **Error**
  - Returns error messages
  - Special errors for authentication requirements

## Technical Details

### SFTPBrowser Class

The core class managing SFTP interactions:

```javascript
class SFTPBrowser {
    // Core properties
    socket              // WebSocket connection
    selectedNode        // Current node ID
    currentPath         // Current directory path
    sessionId           // SFTP session ID
    currentItems        // Array of file items in current directory
    msgId              // Message ID counter
    
    // Methods
    connect()           // Establish SFTP tunnel
    listDirectory()     // Fetch directory listing
    goBack()            // Navigate to parent directory
    refresh()           // Reload current directory
    renderBrowser()     // Render file list UI
    
    // Credentials
    showCredentialsModal()
    submitCredentials()
    cancelCredentials()
    
    // Error handling
    handleError()       // Process errors
    handleTunnelOpened()
    handleListItems()
}
```

### Tab Switching

The `switchTab()` function:
1. Updates current tab state
2. Toggles CSS classes on tab buttons
3. Shows/hides tab content containers
4. Fits terminal on tab switch (for SSH)

### Message Handling

Protocol messages are routed based on `currentTab`:
- SSH tab: Data sent to terminal, resize commands sent
- SFTP tab: List items accumulated, credentials modal shown on auth errors

## CSS Classes Reference

### Tab Navigation
- `.tab-button` - Base tab button styling
- `.tab-button.active` - Active tab highlight
- `.tab-content` - Hidden by default
- `.tab-content.active` - Shows active content

### SFTP Items
- `.sftp-item` - File/folder row container
- `.sftp-item-icon` - Folder 📁 or file 📄 icon
- `.sftp-item-name` - File/folder name (with .sftp-item-dir or .sftp-item-file)
- `.sftp-item-size` - File size display
- `.sftp-item-loading` - Loading/empty state message

### Modal
- `#sftp-credentials-modal` - Credentials input modal
- Shows when authentication required
- Fixed positioning with semi-transparent overlay

## Styling & Theme

The SFTP GUI maintains visual consistency with existing Phirepass UI:

- **Color Scheme**: Dark theme matching SSH terminal
- **Font**: Monospace for paths, sans-serif for labels
- **Accent Color**: Cyan (#22d3ee) for interactive elements
- **Error Color**: Red (#f87171)
- **Background**: Semi-transparent dark panels with subtle borders

## Future Enhancement Opportunities

1. **File Download/Upload**: Add ability to transfer files
2. **File Operations**: Create folders, rename, delete files
3. **Permissions View**: Display file permissions in listing
4. **Search**: Add search functionality within SFTP browser
5. **Bookmarks**: Save favorite directories
6. **Multi-Select**: Select and bulk operations
7. **Preview**: Preview text files directly in browser
8. **Syntax Highlighting**: For code file previews

## Troubleshooting

### SFTP Tab Not Appearing
- Ensure a node is selected first
- Check browser console for JavaScript errors
- Verify server supports SFTP tunnels

### Credentials Modal Not Appearing
- Check that authentication error is being sent by server
- Verify `msg_id` is properly included in responses

### Files Not Loading
- Ensure SFTP tunnel is successfully opened
- Check that path format is correct (use / for root)
- Look for authentication or permission errors in console

### UI Not Responsive
- Verify CSS file is loaded (check network tab)
- Check for JavaScript errors in console
- Ensure viewport height is sufficient (520px minimum)

## Notes for Developers

1. **WASM Integration**: The sftp.js module uses the existing Rust WASM channel (phirepass-channel)
2. **No Backend Changes Required**: All SFTP functionality reuses existing server tunneling
3. **Protocol Version**: Uses existing WebFrameData enums (no new protocol versions)
4. **Browser Compatibility**: Requires ES6 modules and modern JavaScript features
5. **Size Formatting**: File sizes automatically converted to readable units (B, KiB, MiB, GiB)

## Testing Checklist

- [ ] Tab navigation appears after node selection
- [ ] SSH tab works as before
- [ ] SFTP tab shows file listing for root directory
- [ ] Can navigate into directories by clicking folders
- [ ] Back button goes to parent directory
- [ ] Refresh reloads current directory
- [ ] Authentication modal appears when credentials required
- [ ] Credentials can be submitted via Enter or button
- [ ] Cancel button closes modal and disconnects
- [ ] Error messages display properly
- [ ] Switching between tabs works smoothly
- [ ] Terminal resizing works on SSH tab after switching
- [ ] Disconnecting node hides tabs
- [ ] File sizes display correctly for large files
- [ ] Empty directories show "Empty directory" message
- [ ] Special characters in filenames display correctly
