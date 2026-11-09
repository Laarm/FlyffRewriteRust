// The texts of the dashboard (Plan/25 section 2.3: all strings in one place).
export const D = {
  historyNote: (since: string): string =>
    `The history is kept in memory and starts at the last restart (${since} ago).`,
  waitingStatus: 'Waiting for the first status answer...',
  alertsBanner: (n: number): string => (n === 1 ? '1 active alert' : `${String(n)} active alerts`),
  noAlerts: 'No active alert.',
  noAlertsHint: 'The server checks the thresholds every 10 seconds.',
  recentAlerts: 'Recent alert events',
  noRecentAlerts: 'Nothing fired since the last restart.',
  thresholds: 'Thresholds',
  thresholdsHint: 'Set in the [admin.alerts] section of flyff.toml; Reload applies a change.',
  activityEmpty: 'No admin action yet.',
  activityHint:
    'Admin actions and notable server events appear here as they happen (kept since the last restart).',
  quick: 'Quick actions',
  broadcast: 'Broadcast a notice...',
  saveAll: 'Save all characters',
  backupNow: 'Backup now',
  broadcastTitle: 'Broadcast a notice',
  broadcastText: 'Notice text',
  broadcastHint: 'Every player in the world sees it as a server notice.',
  broadcastSend: 'Send notice',
  broadcastDone: 'Notice sent.',
  saveDone: 'Save-all started.',
  backupDone: (name: string): string => `Backup written: ${name}.`,
  needsFeature: 'The activity feed needs a newer server.',
} as const;
