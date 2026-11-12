import { projectLandingMono } from "../../features/monos/model/mono";

type Actions = {
  loadOpenMono: () => string | undefined;
  closeMonoView: () => void;
  cancelNavigation: () => void;
  openProject: (path: string) => void;
  selectProject: (path: string, onRestored: () => void) => void;
  monosEnabled: () => boolean;
  landingEnabled: () => boolean;
  openMono: (id: string) => void;
};

export function selectProjectWithMonoLanding(path: string, actions: Actions) {
  const lastOpenedMono = actions.loadOpenMono();
  actions.closeMonoView();
  actions.cancelNavigation();
  actions.openProject(path);
  actions.selectProject(path, () => {
    const mono = projectLandingMono(
      path,
      lastOpenedMono,
      actions.monosEnabled() && actions.landingEnabled(),
    );
    if (mono) actions.openMono(mono.id);
  });
}
