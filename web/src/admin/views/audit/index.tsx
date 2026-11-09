// The views of the "audit" category (Plan/25 section 2.2, 10.2; WP-ADMIN-29): the Audit log with its filters and exports, and My
// actions with Undo. The History and Logins tabs of the character and account drawers read the same route (`?character=` /
// `?account=`) from the players views.
import type { CategoryModule } from '../../app/views';
import './audit.css';
import { AuditLog } from './AuditLog';
import { MyActions } from './MyActions';

const category: CategoryModule = {
  tabs: {
    log: AuditLog,
    mine: MyActions,
  },
};

export default category;
