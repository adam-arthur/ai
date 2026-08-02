import { mount } from 'svelte'

import App from '#tutor/App.svelte'

mount(App, {
  target: document.querySelector('#app')!,
})
