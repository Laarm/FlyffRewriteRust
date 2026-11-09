// World & Content -> Rates & events (Plan/25 section 7.1): the rates, the event flags, the Event.lua modifiers and the weather on one page.
import { useSnapshot } from '../dashboard/useSnapshot';
import { eventInfo } from './api';
import { EventsSection } from './EventsSection';
import { ModifiersSection } from './ModifiersSection';
import { RatesSection } from './RatesSection';
import { WeatherSection } from './WeatherSection';

export function RatesTab() {
  const info = useSnapshot((signal) => eventInfo(signal), 'event-info', 15_000);
  return (
    <div className="content-tab">
      <RatesSection />
      <EventsSection />
      <ModifiersSection info={info.data} error={info.error} onRetry={info.reload} />
      <WeatherSection info={info.data} onChanged={info.reload} />
    </div>
  );
}
