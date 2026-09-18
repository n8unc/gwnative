/* Embedded in the launcher's private, nonpersistent WKWebView. No network. */
(() => {
  const busyStates = new Set(['queued', 'preparing', 'starting', 'running', 'closing', 'recovering', 'attention', 'unavailable']);
  const model = {
    autoLoginSupported: false,
    busy: (account) => Boolean(account.busy || busyStates.has(account.status)),
    canPlay: (account) => !model.busy(account),
    selectedLaunches: (accounts, selected) => accounts.filter((a) => selected.has(a.id) && model.canPlay(a)).map((a) => a.id),
    passwordEligible: ({ saved, typed, removed, emailChanged }) => !emailChanged && !removed && Boolean(saved || typed),
    savedAutoLogin: ({ editing, adopting, removed = false }) => !removed && Boolean(editing?.autoLogin),
  };
  globalThis.GWLauncherModel = model;
  if (typeof document === 'undefined') return;

  const $ = (id) => document.getElementById(id);
  const selected = new Set();
  const pending = new Map();
  let serial = 0;
  let snapshot = { accounts: [], profiles: [], retained: [] };
  let editing = null;
  let adopting = null;
  let changed = '';
  let polling = false;
  let saving = false;
  let loginChoiceTouched = false;

  function request(action, values = {}) {
    return new Promise((resolve, reject) => {
      const id = ++serial;
      const timeout = setTimeout(() => { pending.delete(id); reject(new Error('The launcher did not respond. Please try again.')); }, 30_000);
      pending.set(id, { resolve, reject, timeout });
      try { window.webkit.messageHandlers.launcher.postMessage(JSON.stringify({ id, action, ...values })); }
      catch { clearTimeout(timeout); pending.delete(id); reject(new Error('The native launcher connection is unavailable.')); }
    });
  }
  window.launcherReply = ({ id, result, error }) => {
    const call = pending.get(id);
    if (!call) return;
    clearTimeout(call.timeout); pending.delete(id);
    error ? call.reject(new Error(error)) : call.resolve(result);
  };
  function notice(message) { $('notice').textContent = message; $('notice').hidden = !message; }
  function element(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  }
  function button(title, run, className = '') {
    const node = element('button', className, title);
    node.type = 'button';
    node.onclick = () => Promise.resolve().then(run).catch((error) => notice(error.message));
    return node;
  }
  async function act(action, values) { await request(action, values); changed = ''; await refresh(); }
  function selection() {
    const ids = model.selectedLaunches(snapshot.accounts, selected);
    $('launch-selected').disabled = !ids.length;
    $('selection-count').textContent = ids.length ? `${ids.length} ready to launch` : 'Select accounts to launch together';
  }
  function toggle(account, field, title) {
    const label = element('label');
    const input = document.createElement('input'); input.type = 'checkbox'; input.checked = account[field];
    input.setAttribute('aria-label', `${title} for ${account.nickname}`);
    input.disabled = field === 'autoLogin' && (!model.autoLoginSupported || !account.hasPassword);
    input.onchange = async () => {
      input.disabled = true;
      try { await act('toggle', { accountId: account.id, field, value: input.checked }); }
      catch (error) { notice(error.message); changed = ''; await refresh(); }
    };
    label.append(input, document.createTextNode(title)); return label;
  }
  function render() {
    const accounts = snapshot.accounts;
    for (const id of selected) if (!accounts.some((a) => a.id === id)) selected.delete(id);
    $('account-count').textContent = `${accounts.length} ACCOUNT${accounts.length === 1 ? '' : 'S'}`;
    $('empty').hidden = accounts.length > 0;
    $('accounts').replaceChildren();
    for (const account of accounts) {
      const row = element('article', `account${selected.has(account.id) ? ' selected' : ''}`);
      const check = document.createElement('input'); check.type = 'checkbox'; check.checked = selected.has(account.id);
      check.setAttribute('aria-label', `Select ${account.nickname}`);
      check.onchange = () => { check.checked ? selected.add(account.id) : selected.delete(account.id); row.classList.toggle('selected', check.checked); selection(); };
      const avatar = element('div', 'avatar', Array.from(account.nickname)[0]?.toUpperCase() || '◇'); avatar.setAttribute('aria-hidden', 'true');
      const details = element('div');
      details.append(element('div', 'account-name', account.nickname), element('div', 'account-email', account.email));
      const status = element('div', `account-status ${account.status === 'running' ? 'active' : account.status === 'failed' || account.status === 'attention' ? 'error' : ''}`, account.statusLabel || 'Ready');
      status.setAttribute('role', 'status'); details.append(status);
      const toggles = element('div', 'row-toggles'); toggles.append(toggle(account, 'autoLogin', 'Auto-login'), toggle(account, 'autoLaunch', 'Auto-launch')); details.append(toggles);
      const actions = element('div', 'row-actions');
      const play = button('Play', () => act('play', { accountIds: [account.id] }), 'primary'); play.disabled = !model.canPlay(account); actions.append(play);
      if (account.status === 'queued') actions.append(button('Cancel', () => act('cancel', { accountId: account.id })));
      if (account.canShow) actions.append(button('Show', () => act('show', { accountId: account.id })));
      if (account.canClose) actions.append(button('Close', () => act('close', { accountId: account.id })));
      if (account.canForceQuit) actions.append(button('Force quit…', async () => {
        if (await confirm('Force quit this game?', 'Unsaved settings or files may be lost. You can cancel and keep waiting.', 'Force quit')) await act('forceQuit', { accountId: account.id });
      }, 'danger'));
      const edit = button('Edit', () => editAccount(account)); edit.setAttribute('aria-label', `Edit ${account.nickname}`); actions.append(edit);
      row.append(check, avatar, details, actions); $('accounts').append(row);
    }
    selection();
  }
  async function refresh() {
    if (polling) return;
    polling = true;
    try {
      const next = await request('snapshot');
      snapshot = next;
      $('update-status').textContent = next.updateMessage || 'Game content shared across all accounts';
      $('check-updates').disabled = Boolean(next.updating);
      const key = JSON.stringify(next.accounts);
      if (key !== changed && !saving) { changed = key; render(); }
    } catch (error) { notice(error.message); }
    finally { polling = false; }
  }
  function eligibility() {
    const originalEmail = editing?.email || adopting?.email;
    const emailChanged = originalEmail && $('email').value.trim().toLowerCase() !== originalEmail.toLowerCase();
    const eligible = model.passwordEligible({ saved: editing?.hasPassword || adopting?.hasPassword, typed: $('password').value.length > 0, removed: $('remove-password').checked, emailChanged });
    $('auto-login').disabled = !model.autoLoginSupported || !eligible;
    if (!eligible && model.autoLoginSupported) $('auto-login').checked = false;
    if (!editing && !adopting && !loginChoiceTouched) $('auto-login').checked = eligible && model.autoLoginSupported;
  }
  function editAccount(account = null, profile = null) {
    editing = account; adopting = profile; loginChoiceTouched = false;
    $('account-form').reset(); $('form-error').hidden = true;
    $('editor-title').textContent = account ? 'Edit Account' : profile ? 'Import Account' : 'Add Account';
    $('nickname').value = account?.nickname || profile?.nickname || '';
    $('email').value = account?.email || profile?.email || '';
    $('auto-login').checked = model.savedAutoLogin({ editing: account, adopting: profile });
    $('auto-launch').checked = Boolean(account?.autoLaunch);
    $('remove-password-row').hidden = !account?.hasPassword;
    $('password-hint').textContent = account?.hasPassword ? 'Leave blank to keep your saved password.' : 'Stored securely in macOS Keychain.';
    const busy = account && model.busy(account);
    for (const id of ['email', 'password', 'remove-password']) $(id).disabled = Boolean(busy);
    $('remove-account').hidden = !account; $('remove-account').disabled = Boolean(busy);
    eligibility(); $('editor').showModal(); $('nickname').focus();
  }
  function confirm(title, message, yes = 'Continue', options = []) {
    return new Promise((resolve) => {
      $('confirm-title').textContent = title; $('confirm-message').textContent = message; $('confirm-yes').textContent = yes;
      $('confirm-options').replaceChildren();
      for (const option of options) {
        const label = element('label', 'check'); const input = document.createElement('input'); input.type = 'checkbox'; input.id = option.id;
        label.append(input, document.createTextNode(option.label)); $('confirm-options').append(label);
      }
      const finish = (accepted) => { const choices = Object.fromEntries(options.map((o) => [o.id, $(o.id).checked])); $('confirmation').close(); resolve(accepted ? choices : null); };
      $('confirm-yes').onclick = () => finish(true); $('confirm-cancel').onclick = () => finish(false);
      $('confirmation').oncancel = (event) => { event.preventDefault(); finish(false); };
      $('confirmation').showModal();
    });
  }
  $('account-form').onsubmit = async (event) => {
    event.preventDefault(); if (saving) return;
    const values = { accountId: editing?.id, profileId: adopting?.profileId, nickname: $('nickname').value.trim(), email: $('email').value.trim(), autoLogin: model.savedAutoLogin({ editing, adopting, removed: $('remove-password').checked }), autoLaunch: $('auto-launch').checked, removePassword: $('remove-password').checked };
    const originalEmail = editing?.email || adopting?.email;
    const emailChanged = originalEmail && values.email.toLowerCase() !== originalEmail.toLowerCase();
    if (emailChanged) {
      if (!await confirm('Change this login?', 'This Account keeps its existing private files. Its old password will be cleared and Auto-login turned off. For a separate game account, use Add Account.')) return;
      values.preserveContext = true; values.autoLogin = false;
    }
    if (!editing && !adopting) {
      const retained = snapshot.retained?.find((a) => a.email.toLowerCase() === values.email.toLowerCase());
      if (retained) {
        const choice = await confirm('Previous settings found', 'Reuse this Account’s previous settings and files, or select Start fresh. Existing files will not be merged or discarded.', 'Continue', [{ id: 'fresh', label: 'Start fresh instead of reusing previous settings' }]);
        if (!choice) return;
        if (!choice.fresh) values.profileId = retained.profileId;
      }
    }
    saving = true; $('save').disabled = true;
    try {
      if ($('password').value && !emailChanged) values.password = $('password').value;
      await request('save', values);
      $('password').value = ''; delete values.password;
      $('editor').close(); notice(''); changed = '';
    } catch (error) { $('form-error').textContent = error.message; $('form-error').hidden = false; }
    finally { delete values.password; saving = false; $('save').disabled = false; await refresh(); }
  };
  $('remove-account').onclick = async () => {
    const result = await confirm('Remove this Account?', 'Saved credentials will be forgotten. Private game settings and files are kept unless you choose to delete them.', 'Remove Account', [{ id: 'delete-files', label: 'Also delete this Account’s private game files' }]);
    if (!result) return;
    try { await act('remove', { accountId: editing.id, deleteFiles: result['delete-files'] }); $('editor').close(); }
    catch (error) { $('form-error').textContent = error.message; $('form-error').hidden = false; }
  };
  $('editor').addEventListener('close', () => { $('password').value = ''; editing = null; adopting = null; });
  $('auto-login').onchange = () => { loginChoiceTouched = true; };
  $('password').oninput = eligibility; $('email').oninput = eligibility; $('remove-password').onchange = eligibility;
  $('cancel-edit').onclick = () => $('editor').close();
  $('add').onclick = () => editAccount(); $('empty-add').onclick = () => editAccount();
  $('launch-selected').onclick = () => act('play', { accountIds: model.selectedLaunches(snapshot.accounts, selected) }).catch((error) => notice(error.message));
  $('check-updates').onclick = () => act('checkUpdates').catch((error) => notice(error.message));
  $('quit-all').onclick = async () => { if (await confirm('Quit launcher and all games?', 'Every running game will be asked to close and save its files. Pending launches will be cancelled.', 'Quit all')) await request('quitAll').catch((error) => notice(error.message)); };
  $('adopt').onclick = async () => {
    try {
      const profiles = await request('profiles'); $('import-list').replaceChildren();
      if (!profiles.length) $('import-list').append(element('p', 'hint', 'No unassigned profiles found.'));
      for (const profile of profiles) {
        const row = element('div', 'import-row'); const info = element('div'); info.append(element('div', '', profile.nickname), element('div', 'hint', profile.email || 'Enter login email during review'));
        row.append(info, button('Review', () => { $('imports').close(); editAccount(null, profile); })); $('import-list').append(row);
      }
      $('imports').showModal();
    } catch (error) { notice(error.message); }
  };
  $('close-imports').onclick = () => $('imports').close();
  refresh(); setInterval(refresh, 1000);
})();
