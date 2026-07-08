import { boot } from 'quasar/wrappers';
import { Notify, Loading, Dialog } from 'quasar';

export default boot(({ app }) => {
  // Configuration des plugins Quasar (sans dark mode par défaut)
  app.use(Notify);
  app.use(Loading);
  app.use(Dialog);

  // Configuration des notifications (style GazFlow)
  Notify.setDefaults({
    position: 'top-right',
    timeout: 3000,
    textColor: 'white',
    backgroundColor: '#2196F3',
    icon: null,
    actions: [{ label: 'OK', color: 'white' }],
  });

  // Configuration du Loading
  Loading.setDefaults({
    spinner: 'QSpinnerGears',
    spinnerColor: '#2196F3',
    spinnerSize: 40,
    backgroundColor: 'rgba(255, 255, 255, 0.9)',
    message: 'Chargement en cours...',
    messageColor: '#212121',
  });
});
