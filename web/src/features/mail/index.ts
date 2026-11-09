// `features/mail` (WP-CFEAT-23): the post window, the inbox, the reader and the send tab. See `mailModule.ts`.
export {
  MODE_MAILBOX,
  QUERY_INTERVAL_MS,
  mailModule,
  mailOf,
  type MailFeature,
} from './mailModule';
export { POST_BUSY, postBusy } from './busy';
export { inCustody, keepColor, keepInfo, keepText, type KeepInfo } from './keep';
export { NO_ATTACHMENT, createMailSenders, type MailSenders } from './senders';
export {
  MAX_MAIL,
  REMOVE,
  applyBox,
  applyPost,
  applyRemove,
  defineMailStore,
  entryOf,
  initialMailState,
  type MailEntry,
  type MailState,
  type MailStore,
} from './state';
export { MailViewBuilder, type MailViewEnv } from './view';
