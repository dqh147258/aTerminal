(() => {
  'use strict';
  const $ = (id) => document.getElementById(id);
  const titles = {users: '用户', devices: '设备', connections: '连接'};
  let token = '', view = 'users', offset = 0, total = 0, sequence = 0, modal = null, busy = false;
  const limit = 25;
  const icons = () => lucide.createIcons();
  const text = (tag, value, className) => {
    const element = document.createElement(tag);
    element.textContent = value;
    if (className) element.className = className;
    return element;
  };
  const date = (seconds) => seconds > 0 ? new Date(seconds * 1000).toLocaleString('zh-CN', {hour12: false}) : '暂无记录';
  const errors = {400: '输入格式有误，请检查后重试。', 401: '管理员令牌无效或已变更，请重新登录。', 403: '请求来源未获授权，请从同源管理页面访问。', 404: '记录已不存在，请刷新列表。', 409: '用户名已存在。', 413: '输入内容过长。', 422: '输入格式有误，请检查后重试。', 429: '操作过于频繁，请稍后重试。'};

  async function api(path, method = 'GET', body) {
    const response = await fetch(`/v2/admin/${path}`, {
      method, credentials: 'omit', cache: 'no-store', redirect: 'error',
      headers: {'Authorization': `Bearer ${token}`, 'X-Admin-Request': '1', ...(body ? {'Content-Type': 'application/json'} : {})},
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!response.ok) {
      if (response.status === 401) logout(errors[401]);
      throw new Error(errors[response.status] || '服务暂时不可用，请稍后刷新重试。');
    }
    return response.status === 204 ? null : response.json();
  }
  function logout(message = '') {
    token = ''; sequence++; modal = null;
    $('dialog').close(); $('dialog-form').reset(); $('token').value = '';
    $('app-view').hidden = true; $('login-view').hidden = false;
    $('table-body').replaceChildren(); $('search').value = ''; $('user-filter').value = '';
    $('notice').hidden = true; $('page-error').hidden = true; $('login-error').textContent = message;
    $('token').focus();
  }
  function showOverview(data) {
    $('count-users').textContent = data.users;
    $('count-devices').textContent = data.devices;
    $('count-online').textContent = data.online_devices;
    $('count-connections').textContent = data.active_connections;
    $('updated').textContent = `更新于 ${date(data.server_time)}`;
  }
  $('login-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const button = event.submitter;
    button.disabled = true; $('login-error').textContent = '';
    token = $('token').value.trim(); $('token').value = '';
    try {
      const data = await api('overview');
      $('login-view').hidden = true; $('app-view').hidden = false;
      showOverview(data); changeView('users');
    } catch (error) { token = ''; $('login-error').textContent = error.message || '无法连接服务。'; }
    finally { button.disabled = false; }
  });
  $('logout').addEventListener('click', () => logout());
  window.addEventListener('pagehide', () => logout());

  function action(icon, label, callback, danger = false, disabled = false) {
    const button = document.createElement('button');
    button.type = 'button'; button.className = danger ? 'danger' : 'quiet';
    button.title = label; button.setAttribute('aria-label', label); button.disabled = disabled;
    const glyph = document.createElement('i'); glyph.dataset.lucide = icon; button.append(glyph);
    button.addEventListener('click', callback); return button;
  }
  function identity(name, id) {
    const cell = document.createElement('td'); cell.append(text('span', name, 'cell-name'));
    const sub = text('span', id, 'sub'); sub.title = id; cell.append(sub); return cell;
  }
  function cell(value, className) { return text('td', value, className); }
  function badge(label, kind) {
    const td = document.createElement('td'); td.append(text('span', label, `badge ${kind}`)); return td;
  }
  function render(items) {
    const headers = {users: ['用户名 / ID', '有效设备', '操作'], devices: ['设备 / ID', '用户', '平台', '状态', '最后在线', '操作'], connections: ['连接 / ID', '用户', '状态', '授权到期', '租约到期', '操作']}[view];
    const tr = document.createElement('tr'); headers.forEach((h) => tr.append(text('th', h))); $('table-head').replaceChildren(tr);
    $('table-body').replaceChildren();
    items.forEach((item) => {
      const row = document.createElement('tr'), actions = document.createElement('div'); actions.className = 'row-actions';
      if (view === 'users') {
        row.append(identity(item.username, item.id), cell(item.device_count));
        actions.append(action('monitor-smartphone', `查看 ${item.username} 的设备`, () => changeView('devices', item.id)), action('key-round', `重置 ${item.username} 的密码`, () => openDialog('reset', item)));
      } else if (view === 'devices') {
        row.append(identity(item.name, item.id), cell(item.username), cell(({desktop: '桌面', ios: 'iOS', android: 'Android'})[item.platform] || item.platform), badge(item.revoked ? '已撤销' : item.online ? '在线' : '离线', item.revoked ? 'bad' : item.online ? 'good' : ''), cell(date(item.last_seen), 'date'));
        actions.append(action('ban', `撤销设备 ${item.name}`, () => openDialog('device', item), true, item.revoked));
      } else {
        const statuses = {revoked: ['已撤销', 'bad'], expired: ['已过期', ''], invalid: ['已失效', 'bad'], connected: ['已连接', 'good'], waiting: ['等待连接', 'pending']};
        row.append(identity(`${item.desktop_name} / ${item.mobile_name}`, item.id), cell(item.username), badge(...(statuses[item.status] || ['未知', ''])), cell(date(item.expires_at), 'date'), cell(date(item.lease_expiry), 'date'));
        actions.append(action('unplug', '撤销连接', () => openDialog('connection', item), true, item.revoked));
      }
      const td = document.createElement('td'); td.append(actions); row.append(td);
      Array.from(row.children).forEach((column, index) => { column.dataset.label = headers[index]; });
      $('table-body').append(row);
    });
    $('empty').hidden = items.length > 0;
    $('empty-title').textContent = $('search').value || $('user-filter').value ? '没有匹配的记录' : `暂无${titles[view]}`;
    document.querySelector('.table-scroll').hidden = items.length === 0;
    $('result-count').textContent = `共 ${total} 条${total ? ` · ${offset + 1}–${Math.min(offset + items.length, total)}` : ''}`;
    $('page-number').textContent = `${Math.floor(offset / limit) + 1} / ${Math.max(1, Math.ceil(total / limit))}`;
    $('previous').disabled = offset === 0; $('next').disabled = offset + limit >= total;
    icons();
  }
  async function refresh() {
    const request = ++sequence;
    $('page-error').hidden = true; $('results').setAttribute('aria-busy', 'true');
    $('empty').hidden = false; $('empty-title').textContent = '正在加载…';
    document.querySelector('.table-scroll').hidden = true;
    $('previous').disabled = true; $('next').disabled = true; $('refresh').disabled = true;
    $('result-count').textContent = ''; $('updated').textContent = '';
    const query = new URLSearchParams({q: $('search').value.trim(), user_id: view === 'users' ? '' : $('user-filter').value.trim(), limit, offset});
    try {
      const [page, overview] = await Promise.all([api(`${view}?${query}`), api('overview')]);
      if (request !== sequence || !token) return;
      total = page.total;
      if (offset >= total && offset > 0) { offset = Math.max(0, Math.floor((total - 1) / limit) * limit); return refresh(); }
      render(page.items); showOverview(overview);
    } catch (error) {
      if (request !== sequence || !token) return;
      $('page-error').textContent = error.message || '无法连接服务，请刷新重试。'; $('page-error').hidden = false;
      $('empty-title').textContent = '加载失败';
    } finally {
      if (request === sequence) { $('refresh').disabled = false; $('results').setAttribute('aria-busy', 'false'); }
    }
  }
  function changeView(nextView, user = '') {
    view = nextView; offset = 0; $('search').value = ''; $('user-filter').value = user;
    $('user-filter').hidden = view === 'users'; $('create-user').hidden = view !== 'users';
    $('view-title').textContent = titles[view]; $('notice').hidden = true;
    $('search').placeholder = view === 'users' ? '搜索用户名' : view === 'devices' ? '搜索设备、用户名或 ID' : '搜索连接、设备或用户名';
    document.querySelectorAll('[data-view]').forEach((button) => { button.classList.toggle('active', button.dataset.view === view); button.setAttribute('aria-current', button.dataset.view === view ? 'page' : 'false'); });
    refresh();
  }
  document.querySelectorAll('[data-view]').forEach((button) => button.addEventListener('click', () => changeView(button.dataset.view)));
  $('filters').addEventListener('submit', (event) => { event.preventDefault(); offset = 0; refresh(); });
  $('clear-filter').addEventListener('click', () => { $('search').value = ''; $('user-filter').value = ''; offset = 0; refresh(); });
  $('refresh').addEventListener('click', refresh);
  $('previous').addEventListener('click', () => { offset = Math.max(0, offset - limit); refresh(); });
  $('next').addEventListener('click', () => { offset += limit; refresh(); });
  $('create-user').addEventListener('click', () => openDialog('create'));

  function openDialog(type, item = {}) {
    modal = {type, item}; $('dialog-form').reset(); $('dialog-error').textContent = '';
    $('username-field').hidden = type !== 'create'; $('username').required = type === 'create';
    const password = type === 'create' || type === 'reset';
    $('password-fields').hidden = !password; $('password').required = password; $('confirm-password').required = password;
    const content = {
      create: ['创建用户', '', '创建用户'],
      reset: ['重置密码', `用户 ${item.username} 的所有设备将退出登录，远程连接将断开，本地 Shell 保留。`, '重置密码'],
      device: ['撤销设备', `撤销 ${item.name} 后，该设备凭据将失效，相关远程连接将断开，本地 Shell 保留。此操作不可恢复，重新登录需生成新设备身份。`, '撤销设备'],
      connection: ['撤销连接', `${item.desktop_name} / ${item.mobile_name} 的远程连接将断开，本地 Shell 保留。`, '撤销连接'],
    }[type];
    $('dialog-title').textContent = content[0]; $('dialog-description').textContent = content[1];
    $('confirm-dialog').querySelector('span').textContent = content[2];
    $('confirm-dialog').className = type === 'create' ? 'primary' : 'danger';
    $('dialog').showModal();
    (type === 'create' ? $('username') : password ? $('password') : $('cancel-dialog')).focus();
  }
  function closeDialog() { if (!busy) { $('dialog').close(); $('dialog-form').reset(); modal = null; } }
  $('close-dialog').addEventListener('click', closeDialog); $('cancel-dialog').addEventListener('click', closeDialog);
  $('dialog').addEventListener('cancel', (event) => { if (busy) event.preventDefault(); });
  $('dialog').addEventListener('close', () => { $('dialog-form').reset(); modal = null; });
  $('dialog-form').addEventListener('submit', async (event) => {
    event.preventDefault(); if (!modal || busy) return;
    const {type, item} = modal;
    const password = $('password').value;
    if (type === 'create' || type === 'reset') {
      const bytes = new TextEncoder().encode(password).length;
      if (bytes < 12 || bytes > 1024) { $('dialog-error').textContent = '密码长度须为 12 至 1024 字节。'; return; }
      if (password !== $('confirm-password').value) { $('dialog-error').textContent = '两次输入的密码不一致。'; return; }
    }
    busy = true; $('confirm-dialog').disabled = true; $('cancel-dialog').disabled = true; $('close-dialog').disabled = true; $('dialog-error').textContent = '';
    try {
      if (type === 'create') await api('users', 'POST', {username: $('username').value, password});
      if (type === 'reset') await api(`users/${encodeURIComponent(item.id)}/password`, 'POST', {password});
      if (type === 'device') await api(`devices/${encodeURIComponent(item.id)}`, 'DELETE');
      if (type === 'connection') await api(`connections/${encodeURIComponent(item.id)}`, 'DELETE');
      $('dialog').close(); $('dialog-form').reset(); modal = null;
      $('notice').textContent = {create: '用户已创建。', reset: '密码已重置，已有登录与连接已失效。', device: '设备已撤销。', connection: '连接已撤销。'}[type]; $('notice').hidden = false;
      await refresh();
    } catch (error) { $('dialog-error').textContent = error.message || '无法连接服务，请稍后重试。'; }
    finally { busy = false; $('confirm-dialog').disabled = false; $('cancel-dialog').disabled = false; $('close-dialog').disabled = false; }
  });
  icons();
})();
