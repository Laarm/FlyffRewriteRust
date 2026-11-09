// The views of the "comm" category (Plan/25 section 2.2, 8; WP-ADMIN-27): Notices (now, scheduled, recurring) and System mail (the grant panel
// in Mail mode with the history of the system mails).
import type { CategoryModule } from '../../app/views';
import { navigate } from '../../stores/route';
import './comm.css';
import { MailTab } from './MailTab';
import { NoticesTab } from './NoticesTab';
import { T } from './strings';

const category: CategoryModule = {
  tabs: {
    notices: NoticesTab,
    mail: MailTab,
  },
  actions: [
    {
      id: 'comm.broadcast',
      label: T.actions.broadcast,
      keywords: ['notice', 'announce', 'sys', 'message', 'everyone'],
      cap: 'act.broadcast',
      icon: 'message',
      run: () => {
        navigate({ category: 'comm', tab: 'notices', params: {} });
      },
    },
    {
      id: 'comm.mail',
      label: T.actions.mail,
      keywords: ['mail', 'post', 'letter', 'system'],
      cap: 'act.items',
      icon: 'message',
      run: () => {
        navigate({ category: 'comm', tab: 'mail', params: {} });
      },
    },
  ],
};

export default category;
