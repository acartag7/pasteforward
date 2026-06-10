import {Composition} from 'remotion';
import {PasteForwardDemo} from './PasteForwardDemo';

export const Root = () => {
  return (
    <Composition
      id="PasteForwardDemo"
      component={PasteForwardDemo}
      durationInFrames={312}
      fps={24}
      width={960}
      height={540}
    />
  );
};
