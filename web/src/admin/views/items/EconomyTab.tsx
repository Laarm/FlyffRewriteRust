// Items & Economy -> Economy (Plan/25 section 5.7): penya by holder, the richest characters and accounts, one counted item over the
// whole world and the grants per day. The server computes it on request (a heavy read) and serves the same answer for ten minutes.
import { useEffect, useState } from 'react';
import { Button, ErrorState, Field, Skeleton, Table, type Column } from '../../components';
import type {
  CensusDto,
  EconomyDto,
  GrantDayDto,
  RichAccountDto,
  RichCharacterDto,
} from '../../gen/dto';
import { fetchEconomy } from '../../grant/api';
import { ensureGrantData } from '../../grant/data';
import { formatNumber } from '../../grant/format';
import { ItemPicker } from '../../grant/ItemPicker';
import { T } from './strings';

function Card({ title, value }: { readonly title: string; readonly value: string }) {
  return (
    <div className="items-card">
      <h3>{title}</h3>
      <div className="items-card__value">{value}</div>
    </div>
  );
}

/** The rows of the by-site table of the census. */
export function siteRows(census: CensusDto): { site: string; pieces: number }[] {
  return Object.entries(census.by_site)
    .map(([site, pieces]) => ({ site, pieces }))
    .sort((a, b) => b.pieces - a.pieces || a.site.localeCompare(b.site));
}

export function EconomyTab() {
  const [item, setItem] = useState<number | null>(null);
  // one request per (item, press of the button); the answer is kept with the request it belongs to
  const [req, setReq] = useState({ n: 0, refresh: false });
  const key = `${String(item)}:${String(req.n)}`;
  const [view, setView] = useState<{ key: string; data: EconomyDto | null; error: unknown }>({
    key: '',
    data: null,
    error: null,
  });
  useEffect(() => {
    void ensureGrantData();
  }, []);
  useEffect(() => {
    let live = true;
    fetchEconomy(item, req.refresh).then(
      (data) => {
        if (live) setView({ key, data, error: null });
      },
      (error: unknown) => {
        if (live) setView({ key, data: null, error });
      },
    );
    return () => {
      live = false;
    };
  }, [item, req, key]);
  const load = (refresh: boolean): void => {
    setReq((r) => ({ n: r.n + 1, refresh }));
  };
  const loading = view.key !== key;

  const charColumns: Column<RichCharacterDto>[] = [
    { id: 'name', header: T.economy.name, cell: (r) => r.name, text: (r) => r.name },
    { id: 'account', header: T.economy.account, cell: (r) => r.account, text: (r) => r.account },
    {
      id: 'gold',
      header: T.economy.wallet,
      cell: (r) => formatNumber(r.gold),
      text: (r) => r.gold,
      align: 'right',
    },
    {
      id: 'bank',
      header: T.economy.bank,
      cell: (r) => formatNumber(r.bank_gold),
      text: (r) => r.bank_gold,
      align: 'right',
    },
  ];
  const accountColumns: Column<RichAccountDto>[] = [
    { id: 'name', header: T.economy.account, cell: (r) => r.name, text: (r) => r.name },
    {
      id: 'wallets',
      header: T.economy.wallet,
      cell: (r) => formatNumber(r.wallets),
      text: (r) => r.wallets,
      align: 'right',
    },
    {
      id: 'banks',
      header: T.economy.bank,
      cell: (r) => formatNumber(r.banks),
      text: (r) => r.banks,
      align: 'right',
    },
    {
      id: 'total',
      header: T.economy.total,
      cell: (r) => formatNumber(r.total),
      text: (r) => r.total,
      align: 'right',
    },
  ];
  const dayColumns: Column<GrantDayDto>[] = [
    { id: 'day', header: T.economy.day, cell: (r) => r.day, text: (r) => r.day },
    {
      id: 'grants',
      header: T.economy.grants,
      cell: (r) => formatNumber(r.grants),
      text: (r) => r.grants,
      align: 'right',
    },
    {
      id: 'goldGrants',
      header: T.economy.goldGrants,
      cell: (r) => formatNumber(r.gold_grants),
      text: (r) => r.gold_grants,
      align: 'right',
    },
    {
      id: 'gold',
      header: T.economy.gold,
      cell: (r) => formatNumber(r.gold),
      text: (r) => r.gold,
      align: 'right',
    },
  ];
  const siteColumns: Column<{ site: string; pieces: number }>[] = [
    { id: 'site', header: T.economy.bySite, cell: (r) => r.site, text: (r) => r.site },
    {
      id: 'pieces',
      header: T.economy.pieces,
      cell: (r) => formatNumber(r.pieces),
      text: (r) => r.pieces,
      align: 'right',
    },
  ];

  const { data, error } = view;
  if (error)
    return (
      <ErrorState
        error={error}
        onRetry={() => {
          load(false);
        }}
      />
    );
  return (
    <div className="items-tab">
      <div className="items-toolbar">
        <Field label={T.economy.item}>
          <ItemPicker value={item} onChange={setItem} label={T.economy.item} placeholder="Perin" />
        </Field>
        <Button
          loading={loading}
          onClick={() => {
            load(true);
          }}
        >
          {T.economy.refresh}
        </Button>
        {data && (
          <span className="grant-hint">
            {T.economy.asOf(
              new Date(data.as_of * 1000).toISOString().slice(0, 16).replace('T', ' '),
              data.cached,
            )}
          </span>
        )}
      </div>
      <p className="grant-hint">{T.economy.hint}</p>
      {data === null ? (
        <>
          {loading && <span className="grant-hint">{T.economy.running}</span>}
          <Skeleton />
        </>
      ) : (
        <>
          <div className="items-cards" data-testid="economy-cards">
            <Card title={T.economy.characters} value={formatNumber(data.characters)} />
            <Card title={T.economy.accounts} value={formatNumber(data.accounts)} />
            <Card title={T.economy.totalPenya} value={formatNumber(data.penya.total)} />
            <Card title={T.economy.wallets} value={formatNumber(data.penya.inventories)} />
            <Card title={T.economy.banks} value={formatNumber(data.penya.banks)} />
            <Card title={T.economy.guildBanks} value={formatNumber(data.penya.guild_banks)} />
            <Card title={T.economy.mails} value={formatNumber(data.penya.mails)} />
            <Card title={T.economy.vendors} value={formatNumber(data.penya.vendors)} />
            <Card title={T.economy.pendingGrants} value={formatNumber(data.penya.pending_grants)} />
          </div>
          <h3 className="grant-h">
            {T.economy.census(
              data.census.name === '' ? `#${String(data.census.item_id)}` : data.census.name,
            )}
          </h3>
          <div className="items-cards">
            <Card title={T.economy.pieces} value={formatNumber(data.census.pieces)} />
            <Card title={T.economy.stacks} value={formatNumber(data.census.stacks)} />
          </div>
          <Table
            columns={siteColumns}
            rows={siteRows(data.census)}
            rowKey={(r) => r.site}
            label={T.economy.bySite}
          />
          <h3 className="grant-h">{T.economy.richestCharacters}</h3>
          <Table
            columns={charColumns}
            rows={data.richest_characters}
            rowKey={(r) => String(r.character_id)}
            label={T.economy.richestCharacters}
            exportName="richest-characters"
          />
          <h3 className="grant-h">{T.economy.richestAccounts}</h3>
          <Table
            columns={accountColumns}
            rows={data.richest_accounts}
            rowKey={(r) => String(r.account_id)}
            label={T.economy.richestAccounts}
            exportName="richest-accounts"
          />
          <h3 className="grant-h">{T.economy.perDay}</h3>
          <Table
            columns={dayColumns}
            rows={data.grants_per_day}
            rowKey={(r) => r.day}
            label={T.economy.perDay}
          />
        </>
      )}
    </div>
  );
}
