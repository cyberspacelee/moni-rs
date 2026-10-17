// Loaded only by e2e's temporary Wrangler config; never included in the deployed Worker.
import Worker, { Hub as RustHub } from './build/index.js';

const bindProxy = (target, overrides) => new Proxy(target, {
  get(object, key) {
    if (Object.hasOwn(overrides, key)) return overrides[key];
    const value = Reflect.get(object, key, object);
    return typeof value === 'function' ? value.bind(object) : value;
  },
});

export default class extends Worker {
  async fetch(req) {
    // The dev proxy can reset early responses with unread bodies (workers-sdk#15203).
    if (req.body) req = new Request(req, { body: await req.arrayBuffer() });
    if (new URL(req.url).pathname.startsWith('/__test/')) {
      return this.env.HUB.getByName('hub').fetch(req);
    }
    const response = await super.fetch(req);
    if (new URL(req.url).pathname === '/api/health') response.headers.set('x-e2e-run', this.env.E2E_RUN_ID);
    return response;
  }
}

export class Hub {
  constructor(state, env) {
    this.state = state;
    this.env = env;
    this.faults = {};
    this.last = {};
    this.alarmSets = 0;
    this.alarmGets = 0;
    const sql = bindProxy(state.storage.sql, {
      exec: (query, ...args) => {
        if (this.faults.sql && query.includes(this.faults.sql)) {
          delete this.faults.sql;
          throw new Error('e2e injected SQL failure');
        }
        const cursor = state.storage.sql.exec(query, ...args);
        this.cursors?.push({ query, cursor });
        return cursor;
      },
    });
    const storage = bindProxy(state.storage, {
      sql,
      setAlarm: (...args) => {
        this.alarmSets++;
        if (this.faults.alarmSets > 0) {
          this.faults.alarmSets--;
          return Promise.reject(new Error('e2e injected setAlarm failure'));
        }
        return state.storage.setAlarm(...args);
      },
      getAlarm: (...args) => {
        this.alarmGets++;
        return state.storage.getAlarm(...args);
      },
    });
    this.wrappedState = bindProxy(state, { storage });
    this.inner = new RustHub(this.wrappedState, env);
  }

  async fetch(req) {
    const path = new URL(req.url).pathname;
    if (!path.startsWith('/__test/')) return this.inner.fetch(req);
    const input = req.method === 'POST' ? await req.json() : {};
    switch (path) {
      case '/__test/sql':
        return Response.json(this.state.storage.sql.exec(input.sql, ...(input.args ?? [])).toArray());
      case '/__test/faults':
        this.faults = input;
        return Response.json('ok');
      case '/__test/recreate':
        this.inner = new RustHub(this.wrappedState, this.env);
        return Response.json('ok');
      case '/__test/delete-alarm':
        await this.state.storage.deleteAlarm();
        return Response.json('ok');
      case '/__test/attachment': {
        const ws = this.state.getWebSockets(`server:${input.id}`).find(ws => !ws.deserializeAttachment().closing);
        const attachment = ws.deserializeAttachment();
        ws.serializeAttachment({ ...attachment, ...input.patch });
        return Response.json('ok');
      }
      case '/__test/alarm':
        try {
          await this.inner.alarm();
          return Response.json({ ok: true });
        } catch {
          return Response.json({ ok: false });
        }
      case '/__test/stats':
        return Response.json({ last: this.last, alarmSets: this.alarmSets,
          alarmGets: this.alarmGets, nextAlarm: await this.state.storage.getAlarm() });
      default:
        return new Response('not found', { status: 404 });
    }
  }

  async webSocketMessage(ws, message) {
    this.cursors = [];
    try {
      await this.inner.webSocketMessage(ws, message);
    } catch {
      // The production callback's rejection is observable through close codes and persisted rows.
    } finally {
      const queries = this.cursors.map(({ query, cursor }) => ({ query,
        rowsRead: cursor.rowsRead, rowsWritten: cursor.rowsWritten }));
      this.last = { message: JSON.parse(message), queries,
        rowsRead: queries.reduce((n, q) => n + q.rowsRead, 0),
        rowsWritten: queries.reduce((n, q) => n + q.rowsWritten, 0) };
      this.cursors = undefined;
    }
  }
  webSocketClose(...args) { return this.inner.webSocketClose(...args); }
  webSocketError(...args) { return this.inner.webSocketError(...args); }
  alarm() { return this.inner.alarm(); }
}
