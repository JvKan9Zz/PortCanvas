import {test} from 'node:test';import assert from 'node:assert/strict';
import {normalizeHex,hexToRgb,rgbToHex,rgbToHsv,hsvToRgb} from '../frontend/color-model.js';
test('hex input is normalized and invalid or injectable input is refused',()=>{assert.equal(normalizeHex(' 3366ff '),'#3366FF');for(const value of ['#123','url(test)','#11223344','GGGGGG'])assert.equal(normalizeHex(value),null);});
test('RGB and HSV preserve selected colors including black, white and primary boundaries',()=>{for(const value of ['#000000','#FFFFFF','#FF0000','#00FF00','#0000FF','#336CE5','#ABCDEF']){const rgb=hexToRgb(value);assert.equal(rgbToHex(rgb),value);assert.deepEqual(hsvToRgb(rgbToHsv(rgb)),rgb);}});
