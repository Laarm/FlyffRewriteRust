// The views of the "server" category (Plan/25 section 2.2; WP-ADMIN-26): Sessions & network, Configuration, Scheduler,
// Backups & maintenance, Logs and Console. Staff & access (`staff`, WP-ADMIN-18) is built by the players views task
// (`StaffAccess.tsx`).
import type { CategoryModule } from '../../app/views';
import './server.css';
import { BackupsAndMaintenance } from './Backups';
import { Config } from './Config';
import { Console } from './Console';
import { Logs } from './Logs';
import { Scheduler } from './Scheduler';
import { Sessions } from './Sessions';
import { StaffAccess } from './StaffAccess';

const category: CategoryModule = {
  tabs: {
    sessions: Sessions,
    staff: StaffAccess,
    config: Config,
    scheduler: Scheduler,
    backups: BackupsAndMaintenance,
    logs: Logs,
    console: Console,
  },
};

export default category;
