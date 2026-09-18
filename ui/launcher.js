/* Embedded in the launcher's private, nonpersistent WKWebView. No network. */
(() => {
  const busyStates = new Set(['queued', 'preparing', 'starting', 'running', 'closing', 'recovering', 'attention', 'unavailable']);
  const model = {
    autoLoginSupported: true,
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
  let groupEditing = null;
  let groupSaving = false;
  let groupMembers = [];
  let enabledPacks = [];

  function orderedRows(container, ids, entries, onChange) {
    container.replaceChildren();
    const ordered = [...ids, ...entries.map(e => e.id).filter(id => !ids.includes(id))];
    for (const id of ordered) {
      const item = entries.find(e => e.id === id);
      const row = element('div', 'ordered-row');
      const check = document.createElement('input'); check.type = 'checkbox'; check.checked = ids.includes(id);
      check.setAttribute('aria-label', `Enable ${item?.name || id}`);
      check.onchange = () => { onChange(check.checked ? [...ids, id] : ids.filter(v => v !== id)); };
      row.append(check, element('span', 'member-name', item?.name || `${id} (missing)`));
      if (ids.includes(id)) {
        const index = ids.indexOf(id);
        for (const [title, delta] of [['↑', -1], ['↓', 1]]) {
          const move = button(title, () => { const next = [...ids]; [next[index], next[index + delta]] = [next[index + delta], next[index]]; onChange(next); });
          move.setAttribute('aria-label', `Move ${item?.name || id} ${delta < 0 ? 'up' : 'down'}`);
          move.disabled = index + delta < 0 || index + delta >= ids.length; row.append(move);
        }
      }
      container.append(row);
    }
  }
  function renderAccountPacks() {
    orderedRows($('account-textures'), enabledPacks, (snapshot.textureLibrary?.packs || []).map(p => ({ id: p.id, name: `${p.name} · ${p.status}` })), ids => { enabledPacks = ids; renderAccountPacks(); });
    if (!enabledPacks.length && !(snapshot.textureLibrary?.packs || []).length) $('account-textures').append(element('p', 'hint', 'No texture packs discovered.'));
    if (enabledPacks.length > 1) {
      const selection = [...enabledPacks];
      request('textureConflicts', {packIds: selection}).then(counts => {
        if (!Array.isArray(counts) || JSON.stringify(selection) !== JSON.stringify(enabledPacks)) return;
        const total = counts.reduce((sum, count) => sum + count, 0);
        if (total) $('account-textures').append(element('p', 'hint', `${total} conflicting replacements use the earlier pack in this order.`));
      }).catch(error => notice(error.message));
    }
  }
  function renderGroupMembers() {
    orderedRows($('group-members'), groupMembers, snapshot.accounts.map(a => ({id: a.id, name: a.nickname})), ids => { groupMembers = ids; renderGroupMembers(); });
  }
  function editGroup(group = null) {
    groupEditing = group; groupMembers = [...(group?.accountIds || [])];
    $('group-name').value = group?.name || ''; $('group-error').hidden = true;
    $('delete-group').hidden = !group; renderGroupMembers(); $('group-editor').showModal(); $('group-name').focus();
  }
  function renderPhaseTwo() {
    $('groups').replaceChildren();
    for (const group of snapshot.groups || []) {
      const row = element('div', 'group-row');
      const names = group.accountIds.map(id => snapshot.accounts.find(a => a.id === id)?.nickname || 'Missing Account');
      const details = element('div', 'group-details'); details.append(element('strong', '', group.name), element('div', 'hint', names.join(' → ') || 'No members'));
      if (group.lastLaunch?.length) details.append(element('div', 'hint', group.lastLaunch.map(member => `${member.name}: ${member.status}`).join(' · ')));
      const launch = button('Launch', async () => {
        await request('launchGroup', {groupId: group.id});
        notice(''); changed = ''; await refresh();
      }, 'primary'); launch.disabled = !group.accountIds.length;
      row.append(details, launch, button('Edit', () => editGroup(group))); $('groups').append(row);
    }
    if (!(snapshot.groups || []).length) $('groups').append(element('p', 'hint', 'Save Accounts in the order you want them to start.'));
    const library = snapshot.textureLibrary || {};
    $('texture-folder').textContent = library.folder || '';
    $('texture-status').textContent = library.message || 'Texture compatibility is checked separately from discovery.';
    $('texture-library').replaceChildren();
    for (const pack of library.packs || []) {
      const row = element('div', 'pack-row'); const details = element('div', 'pack-details');
      const accounts = snapshot.accounts.filter(a => (a.launchPreferences?.texturePackIds || []).includes(pack.id));
      details.append(element('strong', '', pack.name), element('span', 'hint', `${pack.status}${pack.error ? ` · ${pack.error}` : ''}${accounts.length ? ` · Used by ${accounts.map(a => a.nickname).join(', ')}` : ''}`));
      row.append(details); $('texture-library').append(row);
    }
  }
  function launchPreferences() {
    const fps = $('launch-fps').value.trim();
    return { muted: $('launch-sound').value === 'default' ? null : $('launch-sound').value === 'muted', frameRateLimit: fps ? { limit: Number(fps) } : 'default', windowMode: $('launch-mode').value === 'default' ? null : $('launch-mode').value, preferredCharacter: $('preferred-character').value.trim() || null, texturePackIds: enabledPacks };
  }
  function fixedFrame() {
    return $('launch-layout').value === 'fixed' ? Object.fromEntries(['x', 'y', 'width', 'height'].map(key => [key, Number($(`layout-${key}`).value)])) : null;
  }

  function request(action, values = {}) {
    return new Promise((resolve, reject) => {
      const id = ++serial;
      const timeout = action === 'changeTextureFolder' ? null : setTimeout(() => { pending.delete(id); reject(new Error('The launcher did not respond. Please try again.')); }, 30_000);
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
  async function act(action, values) { const result = await request(action, values); if (result?.members) notice(result.members.map(m => `${m.name}: ${m.status}`).join(' · ')); changed = ''; await refresh(); }
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
      if (account.launchWarning) details.append(element('div', 'account-status error', account.launchWarning));
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
    renderPhaseTwo();
  }
  async function refresh() {
    if (polling) return;
    polling = true;
    try {
      const next = await request('snapshot');
      snapshot = next;
      $('update-status').textContent = next.updateMessage || 'Game content shared across all accounts';
      $('check-updates').disabled = Boolean(next.updating);
      const key = JSON.stringify([next.accounts, next.groups, next.textureLibrary]);
      if (key !== changed && !saving) { changed = key; render(); if ($('editor').open) renderAccountPacks(); }
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
    const prefs = account?.launchPreferences || {};
    $('launch-sound').value = prefs.muted == null ? 'default' : prefs.muted ? 'muted' : 'on';
    $('launch-fps').value = prefs.frameRateLimit?.limit ? String(prefs.frameRateLimit.limit) : '';
    $('launch-mode').value = prefs.windowMode || 'default';
    $('preferred-character').value = prefs.preferredCharacter || '';
    $('last-seen-characters').replaceChildren(...(account?.lastSeenCharacters || []).map(name => {
      const option = document.createElement('option'); option.value = name; option.label = `Last seen — ${name}`; return option;
    }));
    const frame = account?.windowPreferences?.fixedLaunchFrame;
    $('launch-layout').value = frame ? 'fixed' : 'restore'; $('layout-values').hidden = !frame;
    for (const key of ['x', 'y', 'width', 'height']) $(`layout-${key}`).value = String(frame?.[key] ?? ({x: 0, y: 0, width: 1280, height: 800}[key]));
    $('capture-layout').disabled = account?.status !== 'running';
    $('next-launch-hint').textContent = account && model.busy(account) ? 'Game is open or queued. These edits apply to its next launch.' : 'Applied on next launch.';
    enabledPacks = [...(prefs.texturePackIds || [])]; renderAccountPacks();
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
    const originalEmail = editing?.email || adopting?.email;
    const email = $('email').value.trim();
    const removePassword = $('remove-password').checked;
    const emailChanged = originalEmail && email.toLowerCase() !== originalEmail.toLowerCase();
    const values = { accountId: editing?.id, profileId: adopting?.profileId, nickname: $('nickname').value.trim(), email, autoLogin: !removePassword && !emailChanged && $('auto-login').checked, autoLaunch: $('auto-launch').checked, removePassword };
    values.launchPreferences = launchPreferences(); values.fixedLaunchFrame = fixedFrame();
    if (emailChanged) {
      if (!await confirm('Change this login?', 'This Account keeps its existing private files. Its old password will be cleared and Auto-login turned off. For a separate game account, use Add Account.')) return;
      values.preserveContext = true;
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
  $('launch-layout').onchange = () => { $('layout-values').hidden = $('launch-layout').value !== 'fixed'; };
  $('capture-layout').onclick = async () => {
    const accountId = editing?.id; if (!accountId) return;
    $('capture-layout').disabled = true;
    try {
      const result = await request('captureLayout', {accountId});
      if (editing?.id !== accountId || !$('editor').open) return;
      const frame = result.frame;
      if (!frame) throw new Error('Window layout capture is pending. Try again once the game responds.');
      $('launch-layout').value = 'fixed'; $('layout-values').hidden = false;
      for (const key of ['x', 'y', 'width', 'height']) $(`layout-${key}`).value = String(frame[key]);
    } catch (error) { if (editing?.id === accountId) { $('form-error').textContent = error.message; $('form-error').hidden = false; } }
    finally { if (editing?.id === accountId) $('capture-layout').disabled = snapshot.accounts.find(a => a.id === accountId)?.status !== 'running'; }
  };
  $('new-group').onclick = () => editGroup();
  $('cancel-group').onclick = () => $('group-editor').close();
  $('group-form').onsubmit = async event => {
    event.preventDefault(); if (groupSaving) return; groupSaving = true; $('save-group').disabled = true;
    try { await act('saveGroup', {groupId: groupEditing?.id, name: $('group-name').value.trim(), accountIds: groupMembers}); $('group-editor').close(); }
    catch (error) { $('group-error').textContent = error.message; $('group-error').hidden = false; }
    finally { groupSaving = false; $('save-group').disabled = false; }
  };
  $('delete-group').onclick = async () => {
    if (!await confirm('Delete this group?', 'Accounts and running games are kept.', 'Delete group')) return;
    try { await act('deleteGroup', {groupId: groupEditing.id}); $('group-editor').close(); }
    catch (error) { $('group-error').textContent = error.message; $('group-error').hidden = false; }
  };
  $('refresh-textures').onclick = () => act('refreshTextures').catch(error => notice(error.message));
  $('open-textures').onclick = () => act('openTextureFolder').catch(error => notice(error.message));
  $('change-textures').onclick = () => act('changeTextureFolder').catch(error => notice(error.message));
  refresh(); setInterval(refresh, 1000);
})();
