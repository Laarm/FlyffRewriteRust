// The views of the "dashboard" category (Plan/25 section 2.2; WP-ADMIN-25): Overview (cards, charts, quick actions), Alerts and
// Activity. Everything is read from `GET /admin/api/{history,alerts,activity}` and the live status of the shell.
import type { CategoryModule } from '../../app/views';
import './dashboard.css';
import { Activity } from './Activity';
import { Alerts } from './Alerts';
import { Overview } from './Overview';

const category: CategoryModule = {
  tabs: { overview: Overview, alerts: Alerts, activity: Activity },
};

export default category;
