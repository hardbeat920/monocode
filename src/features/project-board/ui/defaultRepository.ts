import {
  projectBoardRepository,
  subscribeProjectBoard,
  dispatchProjectBoardChanged,
} from "../model/projectBoard";

export const defaultProjectBoardRepository = projectBoardRepository;
export { subscribeProjectBoard, dispatchProjectBoardChanged };
