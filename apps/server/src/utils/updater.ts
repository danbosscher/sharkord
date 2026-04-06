class Updater {
  public canUpdate = (): boolean => false;

  public getLatestVersion = async () => '0.0.0';

  public hasUpdates = async () => false;

  public update = async (): Promise<void> => {};
}

const updater = new Updater();

export { updater };
