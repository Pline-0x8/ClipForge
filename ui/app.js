/* Clipboard writes and focus remain native; edits are pinned until committed. */
(() => {
  const {invoke}=window.__TAURI__.core, {listen}=window.__TAURI__.event;
  const $=id=>document.getElementById(id);
  let state=null,lastContent='',lastSelection=null,dragging=false,pending=null,editor=null,saving=null,opening=null,outsidePointer=false;
  async function command(name,args={}) {try{return await invoke(name,args);}catch(error){$('status').textContent=String(error);}}
  const summary=text=>text.split(/\r?\n/).slice(0,2).map(line=>Array.from(line).slice(0,140).join('')).join('\n')||'(Empty text)';
  function refresh(){if(outsidePointer)return;if(pending&&!editor&&!dragging){const next=pending;pending=null;lastContent='';render(next);}else if(state&&!editor&&!dragging){lastContent='';render(state);}}
  function finishEditor(){if(!editor)return;editor=null;command('end_edit');refresh();}
  async function saveEditor(){
    if(opening)await opening;
    if(saving)return saving;
    if(!editor)return true;
    const active=editor;
    saving=(async()=>{
      const name=active.name?.value||'',value=active.text.value;
      const text=value===active.original.replace(/\r\n?/g,'\n')?active.original:value;
      if(Array.from(name).length>80){active.error.textContent='Names must be 80 characters or fewer.';return false;}
      if(new TextEncoder().encode(text).length>1024*1024){active.error.textContent='Text must be 1 MiB or smaller.';return false;}
      try{
        if(text===active.original&&name===active.originalName){finishEditor();return true;}
        active.text.disabled=true;active.submit.disabled=true;if(active.name)active.name.disabled=true;
        if(active.register===null)await invoke('set_clipboard',{text});
        else await invoke('edit_register',{register:active.register,name,text});
        finishEditor();return true;
      }catch(error){active.text.disabled=false;active.submit.disabled=false;if(active.name)active.name.disabled=false;active.error.textContent=String(error);return false;}
    })();
    try{return await saving;}finally{saving=null;}
  }
  async function action(name,args={}){if(await saveEditor())return command(name,args);}
  async function openEditor(register,host){
    if(editor?.register===register)return;
    if(!(await saveEditor()))return;
    opening=(async()=>{
      try{
        await invoke('begin_edit');
        const index=register===null?-1:register.charCodeAt(0)-97;
        const original=(register===null?state.currentClipboard:state.registers[index])??'';
        if(register!==null&&state.registers[index]!==null)await invoke('set_clipboard',{text:original});
        const box=document.createElement('div');box.className='inline-editor';
        let name=null;
        if(register!==null){name=document.createElement('input');name.className='inline-name';name.maxLength=80;name.placeholder='Optional name';name.value=state.registerNames?.[index]||'';name.setAttribute('aria-label',`Register ${register} name`);box.append(name);}
        const text=document.createElement('textarea');text.className='inline-text';text.value=original;text.rows=5;text.spellcheck=false;text.setAttribute('aria-label',register===null?'Current clipboard text':`Register ${register} text`);
        const hint=document.createElement('p');hint.className='inline-hint';hint.textContent='Submit or click elsewhere to save · Esc to cancel';
        const error=document.createElement('p');error.className='error';error.setAttribute('role','alert');
        const submit=document.createElement('button');submit.type='button';submit.className='inline-submit';submit.textContent='Submit';submit.setAttribute('aria-label',register===null?'Save current clipboard edits':`Save register ${register} edits`);
        submit.addEventListener('click',event=>{event.stopPropagation();return saveEditor();});
        if(register!==null)host=Array.from($('registers').children).find(row=>row.dataset.register===String(index)).children[0];
        else host=$('current-preview');
        box.append(text,hint,error,submit);host.replaceChildren(box);
        editor={register,host,box,name,text,error,submit,original,originalName:name?.value||''};
        $(register===null?'current-clipboard':'registers').draggable=false;
        text.focus();
      }catch(error){$('status').textContent=String(error);command('end_edit');}
    })();
    try{await opening;}finally{opening=null;}
  }
  function row(text,label,empty=false){
    const button=document.createElement('button');button.className=`row${empty?' empty':''}`;button.setAttribute('role','listitem');
    const key=document.createElement('span');key.className='key';key.textContent=label;
    const content=document.createElement('span');content.className='content';
    const preview=document.createElement('span');preview.className='preview';preview.textContent=empty?'Empty register':summary(text);
    content.append(preview);button.append(key,content);return button;
  }
  function render(snapshot){
    state=snapshot;
    $('status').textContent=snapshot.status||'Ready';
    $('mode').textContent=snapshot.copy?'Choose a register to save into':'Choose clipboard text';
    $('hint').textContent=snapshot.copy?'Press a letter to save the copied selection.':'Click a cell to edit. Click elsewhere to save. Drag history to a cell.';
    if(editor||dragging||opening||outsidePointer){pending=snapshot;return;}
    $('current-preview').textContent=snapshot.currentClipboard==null?'No text on clipboard':summary(snapshot.currentClipboard);
    $('current-clipboard').draggable=snapshot.currentClipboard!=null;
    const signature=JSON.stringify([snapshot.registers,snapshot.registerNames,snapshot.history]);
    if(signature!==lastContent){
      lastContent=signature;$('registers').replaceChildren();
      snapshot.registers.map((text,index)=>({text,index})).sort((a,b)=>Number(b.text!==null)-Number(a.text!==null)||a.index-b.index).forEach(({text,index})=>{
        const letter=String.fromCharCode(97+index),name=snapshot.registerNames?.[index]||'';
        const original=row(text,letter.toUpperCase(),text===null),container=document.createElement('div');container.className=original.className;container.setAttribute('role','listitem');container.dataset.register=String(index);
        if(text!==null)container.dataset.selection=String(snapshot.registers.slice(0,index).filter(value=>value!==null).length);
        const cell=document.createElement('div');cell.className='register-cell';
        const content=document.createElement('button');content.className='register-edit';content.setAttribute('aria-label',`Edit register ${letter}`);content.append(...original.children);
        if(name){const title=document.createElement('span');title.className='register-name';title.textContent=name;content.children[1].prepend(title);}
        content.addEventListener('click',()=>openEditor(letter,cell));cell.append(content);container.append(cell);
        if(text!==null||name){const clear=document.createElement('button');clear.className='row-action clear-register';clear.textContent='🗑';clear.setAttribute('aria-label',`Clear register ${letter}`);clear.addEventListener('click',()=>action('clear_register',{register:letter}));container.append(clear);}
        dropTarget(container,text=>action('save_text',{register:letter,text}));$('registers').append(container);
      });
      $('history').replaceChildren();
      snapshot.history.forEach((text,index)=>{
        const button=row(text,String(index+1).padStart(2,'0'));button.classList.add('history-row');button.draggable=true;button.dataset.selection=String(snapshot.registers.filter(value=>value!==null).length+index);
        button.addEventListener('click',()=>action('set_clipboard',{text}));dragSource(button,()=>text);$('history').append(button);
      });
      if(!snapshot.history.length){const empty=document.createElement('div');empty.className='empty-state';empty.textContent='Your next copy starts the ring. Recent text will appear here.';$('history').append(empty);}
      $('count').textContent=`${snapshot.history.length} items`;
    }
    document.querySelectorAll('.row').forEach(button=>{const selected=snapshot.selection!==null&&Number(button.dataset.selection??-1)===snapshot.selection;button.classList.toggle('selected',selected);if(selected&&snapshot.selection!==lastSelection)button.scrollIntoView({block:'nearest'});});lastSelection=snapshot.selection;
  }
  function dropTarget(element,handler){
    element.addEventListener('dragover',event=>{if(!event.dataTransfer.types.includes('text/plain'))return;event.preventDefault();event.dataTransfer.dropEffect='copy';element.classList.add('drag-over');});
    element.addEventListener('dragleave',()=>element.classList.remove('drag-over'));
    element.addEventListener('drop',event=>{event.preventDefault();element.classList.remove('drag-over');if(event.dataTransfer.types.includes('text/plain'))return handler(event.dataTransfer.getData('text/plain'));});
  }
  function dragSource(element,getText){
    element.addEventListener('dragstart',event=>{if((editor&&element.contains(editor.box))||getText()==null){event.preventDefault();return;}dragging=true;event.dataTransfer.setData('text/plain',getText());event.dataTransfer.effectAllowed='copy';});
    element.addEventListener('dragend',()=>{dragging=false;outsidePointer=false;refresh();});
  }
  $('hide').onclick=()=>action('dismiss',{commit:false});$('clear').onclick=()=>action('clear_all');$('quit').onclick=()=>action('quit');
  $('current-clipboard').addEventListener('click',event=>{if(editor?.register===null&&editor.box.contains(event.target))return;return openEditor(null,$('current-preview'));});
  dropTarget($('current-clipboard'),text=>action('set_clipboard',{text}));dragSource($('current-clipboard'),()=>state?.currentClipboard);
  document.addEventListener('pointerdown',event=>{if(editor&&!editor.box.contains(event.target)){outsidePointer=true;saveEditor();}});
  document.addEventListener('click',()=>{if(outsidePointer){outsidePointer=false;setTimeout(refresh,0);}});
  document.addEventListener('pointercancel',()=>{if(!dragging){outsidePointer=false;setTimeout(refresh,0);}});
  $('save-current').onclick=async()=>{if(!(await saveEditor()))return;try{await invoke('begin_edit');$('save-error').textContent='';$('save-dialog').showModal();$('save-letter').value='';$('save-letter').focus();}catch(error){$('status').textContent=String(error);}};
  function cancelSave(){ $('save-dialog').close();command('end_edit'); }
  $('cancel-save').onclick=cancelSave;$('save-dialog').addEventListener('cancel',event=>{event.preventDefault();cancelSave();});
  $('save-form').onsubmit=async event=>{event.preventDefault();const register=$('save-letter').value.toLowerCase();if(/^[a-z]$/.test(register)){try{await invoke('save_current',{register});cancelSave();}catch(error){$('save-error').textContent=String(error);}}};
  function cancelEdit(){if(saving)return;if(editor){finishEditor();}else if($('save-dialog').open)cancelSave();}
  document.addEventListener('keydown',event=>{
    if(editor){if(event.key==='Escape'){event.preventDefault();cancelEdit();}return;}
    if($('save-dialog').open||event.target.matches('input,textarea'))return;
    if(event.key==='Escape'){event.preventDefault();command('dismiss',{commit:false});}
    else if(event.key==='Enter'){event.preventDefault();action('dismiss',{commit:true});}
    else if(['Tab','ArrowDown','ArrowUp','ArrowLeft','ArrowRight'].includes(event.key)){event.preventDefault();command('navigate',{backwards:event.shiftKey||['ArrowUp','ArrowLeft'].includes(event.key)});}
    else if(/^[a-z]$/i.test(event.key)&&!event.metaKey){event.preventDefault();if(!event.repeat)command('select_register',{register:event.key.toLowerCase()});}
  });
  listen('clipforge-toggle',()=>{if($('save-dialog').open)cancelSave();return action('dismiss',{commit:false});}).catch(error=>{$('status').textContent=String(error);});
  listen('clipforge-cancel-edit',cancelEdit).catch(error=>{$('status').textContent=String(error);});
  listen('clipforge-state',event=>render(event.payload)).then(()=>invoke('snapshot')).then(render).catch(error=>{$('status').textContent=String(error);});
})();
